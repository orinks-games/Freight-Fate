// SPDX-License-Identifier: MPL-2.0
//
// Freight Fate's modified copy of Prism 0.1.6's Android text to speech
// backend (github.com/ethindp/prism), compiled into the APK in place of
// Prism's own (see JAVA_OVERRIDES in crates/ff-tools/src/release/android.rs).
// Changes from Prism: the rate and pitch scales are centred on Android's
// normal 1.0; the voice list holds every installed engine's voices, named
// "engine: language: voice"; and choosing another engine's voice moves
// speech to that engine.
package com.github.ethindp.prism;

import android.content.Context;
import android.content.Intent;
import android.media.AudioAttributes;
import android.os.Build;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.speech.tts.*;
import android.speech.tts.TextToSpeech.*;
import android.system.ErrnoException;
import android.system.Os;
import android.system.OsConstants;
import com.snapchat.djinni.Outcome;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.nio.*;
import java.nio.charset.*;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.EnumSet;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;

public class AndroidTextToSpeechBackend extends TextToSpeechBackend implements AutoCloseable {
  private static final long INITIALIZATION_TIMEOUT_SECONDS = 10;

  private volatile TextToSpeech tts;
  // Package name of the engine tts is connected to.
  private volatile String activeEngine;
  private float ttsVolume = 1.0f;
  private float ttsRate = 1.0f;
  private float ttsPitch = 1.0f;
  private volatile boolean isTTSInitialized = false;
  private final Object ttsStateLock = new Object();
  private CharsetDecoder decoder;
  // The voices of every installed engine. Null until a voice operation needs it, because building
  // it connects to each engine.
  private List<VoiceEntry> voiceList;
  private ConcurrentHashMap<String, Consumer<Integer>> pendingUtterances =
      new ConcurrentHashMap<>();

  @Override
  public String getName() {
    return "Android Text to Speech";
  }

  @Override
  public EnumSet<BackendFeaturesHigh> getFeaturesHigh() {
    return EnumSet.of(BackendFeaturesHigh.IS_SPEECH_ENGINE);
  }

  @Override
  public EnumSet<BackendFeatures> getFeatures() {
    final EnumSet<BackendFeatures> features = EnumSet.noneOf(BackendFeatures.class);
    try {
      final TextToSpeech localTts = this.tts;
      final boolean initialized = this.isTTSInitialized;
      final boolean available;
      if (localTts != null) {
        available = initialized;
      } else {
        boolean engineInstalled = false;
        try {
          final var ctx = PrismContext.get();
          if (ctx != null) {
            final var pm = ctx.getPackageManager();
            if (pm != null) {
              final var probe = new Intent(TextToSpeech.Engine.INTENT_ACTION_TTS_SERVICE);
              engineInstalled = !pm.queryIntentServices(probe, 0).isEmpty();
            }
          }
        } catch (Exception e) {
          engineInstalled = false;
        }
        available = engineInstalled;
      }
      if (available) {
        features.add(BackendFeatures.IS_SUPPORTED_AT_RUNTIME);
      }
      features.add(BackendFeatures.SUPPORTS_SPEAK);
      features.add(BackendFeatures.SUPPORTS_OUTPUT);
      features.add(BackendFeatures.SUPPORTS_SPEAK_TO_MEMORY);
      features.add(BackendFeatures.SUPPORTS_IS_SPEAKING);
      features.add(BackendFeatures.SUPPORTS_STOP);
      features.add(BackendFeatures.SUPPORTS_SET_VOLUME);
      features.add(BackendFeatures.SUPPORTS_GET_VOLUME);
      features.add(BackendFeatures.SUPPORTS_SET_RATE);
      features.add(BackendFeatures.SUPPORTS_GET_RATE);
      features.add(BackendFeatures.SUPPORTS_SET_PITCH);
      features.add(BackendFeatures.SUPPORTS_GET_PITCH);
      features.add(BackendFeatures.SUPPORTS_REFRESH_VOICES);
      features.add(BackendFeatures.SUPPORTS_COUNT_VOICES);
      features.add(BackendFeatures.SUPPORTS_GET_VOICE_NAME);
      features.add(BackendFeatures.SUPPORTS_GET_VOICE_LANGUAGE);
      features.add(BackendFeatures.SUPPORTS_GET_VOICE);
      features.add(BackendFeatures.SUPPORTS_SET_VOICE);
      return features;
    } catch (Exception e) {
      return features;
    }
  }

  @Override
  public Outcome<Unit, BackendError> initialize() {
    synchronized (ttsStateLock) {
      if (isTTSInitialized || tts != null) {
        return Outcome.fromError(BackendError.ALREADY_INITIALIZED);
      }
    }
    final CharsetDecoder candidateDecoder =
        StandardCharsets.UTF_8
            .newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT);
    final var ctx = PrismContext.get();
    if (ctx == null) {
      return Outcome.fromError(BackendError.BACKEND_NOT_AVAILABLE);
    }
    TtsKeepAlive.start(ctx);
    final TextToSpeech candidate = new EngineConnection(ctx, null).await(initializationDeadline());
    if (candidate == null) {
      return Outcome.fromError(BackendError.BACKEND_NOT_AVAILABLE);
    }
    try {
      try {
        candidate.setLanguage(Locale.getDefault());
      } catch (RuntimeException ignored) {
      }
      configureEngine(candidate, 1.0f, 1.0f);
      final String candidateEngine = candidate.getDefaultEngine();
      synchronized (ttsStateLock) {
        decoder = candidateDecoder;
        voiceList = null;
        activeEngine = candidateEngine;
        tts = candidate;
        isTTSInitialized = true;
      }
      return Outcome.fromResult(new Unit());
    } catch (RuntimeException e) {
      shutdownEngine(candidate);
      return Outcome.fromError(BackendError.BACKEND_NOT_AVAILABLE);
    }
  }

  private void configureEngine(TextToSpeech engine, float rate, float pitch) {
    engine.setPitch(pitch);
    engine.setSpeechRate(rate);
    final AudioAttributes audioAttributes =
        new AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_ACCESSIBILITY)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
            .build();
    engine.setAudioAttributes(audioAttributes);
    engine.setOnUtteranceProgressListener(
        new UtteranceProgressListener() {
          @Override
          public void onStart(String utteranceId) {}

          @Override
          public void onDone(String utteranceId) {
            final Consumer<Integer> callback = pendingUtterances.get(utteranceId);
            if (callback != null) {
              callback.accept(TextToSpeech.SUCCESS);
            }
          }

          @Override
          public void onError(String utteranceId) {
            final Consumer<Integer> callback = pendingUtterances.get(utteranceId);
            if (callback != null) {
              callback.accept(TextToSpeech.ERROR);
            }
          }
        });
  }

  @Override
  public Outcome<Unit, BackendError> speak(ByteBuffer text, boolean interrupt) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    var bb = text.asReadOnlyBuffer();
    decoder.reset();
    var out = CharBuffer.allocate((int) Math.ceil(bb.capacity() * decoder.maxCharsPerByte()));
    var cr = decoder.decode(bb, out, true);
    if (cr.isOverflow()) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    cr = decoder.flush(out);
    if (cr.isOverflow()) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    out.flip();
    Bundle params = new Bundle();
    params.putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, ttsVolume);
    if (out.remaining() >= TextToSpeech.getMaxSpeechInputLength()) {
      var segments =
          TextChunker.split(out, out.remaining(), TextToSpeech.getMaxSpeechInputLength());
      for (int segment = 0; segment < segments.size(); ++segment) {
        if (segment == 0 && interrupt) {
          if (tts.speak(segments.get(segment), TextToSpeech.QUEUE_FLUSH, params, null)
              != TextToSpeech.SUCCESS) return Outcome.fromError(BackendError.SPEAK_FAILURE);
        } else {
          if (tts.speak(segments.get(segment), TextToSpeech.QUEUE_ADD, params, null)
              != TextToSpeech.SUCCESS) return Outcome.fromError(BackendError.SPEAK_FAILURE);
        }
      }
    } else {
      if (tts.speak(
              out.toString(),
              interrupt ? TextToSpeech.QUEUE_FLUSH : TextToSpeech.QUEUE_ADD,
              params,
              null)
          != TextToSpeech.SUCCESS) return Outcome.fromError(BackendError.SPEAK_FAILURE);
    }
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Unit, BackendError> output(ByteBuffer text, boolean interrupt) {
    return speak(text, interrupt);
  }

  @Override
  public Outcome<Unit, BackendError> speakToMemory(
      ByteBuffer text, AudioCallback callback, long userdata) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    var bb = text.asReadOnlyBuffer();
    decoder.reset();
    var out = CharBuffer.allocate((int) Math.ceil(bb.capacity() * decoder.maxCharsPerByte()));
    var cr = decoder.decode(bb, out, true);
    if (cr.isOverflow()) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    cr = decoder.flush(out);
    if (cr.isOverflow()) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    out.flip();
    String textString = out.toString();
    ParcelFileDescriptor pfd;
    File tempFile = null;
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      try {
        var memfd = Os.memfd_create("tts_synthesis", 0);
        pfd = ParcelFileDescriptor.dup(memfd);
        Os.close(memfd);
      } catch (ErrnoException | IOException e) {
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
    } else {
      try {
        tempFile = File.createTempFile("prism_tts_", ".wav", PrismContext.get().getCacheDir());
        pfd = ParcelFileDescriptor.open(tempFile, ParcelFileDescriptor.MODE_READ_WRITE);
      } catch (IOException e) {
        if (tempFile != null) tempFile.delete();
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
    }
    String utteranceId = UUID.randomUUID().toString();
    CountDownLatch latch = new CountDownLatch(1);
    final AtomicBoolean success = new AtomicBoolean(false);
    try {
      pendingUtterances.put(
          utteranceId,
          (status) -> {
            if (status == TextToSpeech.SUCCESS) success.set(true);
            latch.countDown();
          });
      Bundle params = new Bundle();
      params.putFloat(TextToSpeech.Engine.KEY_PARAM_VOLUME, ttsVolume);
      int result;
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        result = tts.synthesizeToFile(textString, params, pfd, utteranceId);
      } else {
        result = tts.synthesizeToFile(textString, params, tempFile, utteranceId);
      }
      if (result != TextToSpeech.SUCCESS) {
        return Outcome.fromError(BackendError.SPEAK_FAILURE);
      }
      try {
        if (!latch.await(60, TimeUnit.SECONDS)) {
          return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
        }
      } catch (InterruptedException e) {
        Thread.currentThread().interrupt();
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
      if (!success.get()) {
        return Outcome.fromError(BackendError.SPEAK_FAILURE);
      }
      try {
        Os.lseek(pfd.getFileDescriptor(), 0, OsConstants.SEEK_SET);
      } catch (ErrnoException e) {
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
      try (FileInputStream fis = new FileInputStream(pfd.getFileDescriptor())) {
        byte[] header = new byte[44];
        if (fis.read(header) != 44) {
          return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
        }
        ByteBuffer headerBuf = ByteBuffer.wrap(header).order(ByteOrder.LITTLE_ENDIAN);
        short channels = headerBuf.getShort(22);
        int sampleRate = headerBuf.getInt(24);
        short bitsPerSample = headerBuf.getShort(34);
        if (bitsPerSample != 16) {
          return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
        }
        byte[] data;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
          data = fis.readAllBytes();
        } else {
          int dataSize = (int) (pfd.getStatSize() - 44);
          data = new byte[dataSize];
          int totalRead = 0;
          while (totalRead < dataSize) {
            int read = fis.read(data, totalRead, dataSize - totalRead);
            if (read == -1) break;
            totalRead += read;
          }
        }
        ByteBuffer pcmBuf = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN);
        ShortBuffer shortBuf = pcmBuf.asShortBuffer();
        int sampleCount = shortBuf.remaining();
        ByteBuffer floatBytes = ByteBuffer.allocate(sampleCount * 4).order(ByteOrder.nativeOrder());
        FloatBuffer floatBuf = floatBytes.asFloatBuffer();
        for (int i = 0; i < sampleCount; i++) {
          float sample = shortBuf.get(i) / 32768.0f;
          floatBuf.put(sample);
        }
        callback.onAudio(userdata, floatBytes, sampleCount, channels, sampleRate);
      } catch (IOException e) {
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
      return Outcome.fromResult(new Unit());
    } finally {
      pendingUtterances.remove(utteranceId);
      try {
        pfd.close();
      } catch (IOException ignored) {
      }
      if (tempFile != null) tempFile.delete();
    }
  }

  @Override
  public Outcome<Boolean, BackendError> isSpeaking() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    return Outcome.fromResult(tts.isSpeaking());
  }

  @Override
  public Outcome<Unit, BackendError> stop() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    if (tts.stop() != TextToSpeech.SUCCESS)
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    pendingUtterances.clear();
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Unit, BackendError> setRate(float rate) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    ttsRate = Utils.expRangeConvert(rate, 0.1f, 1.0f, 6.0f);
    if (tts.setSpeechRate(ttsRate) == TextToSpeech.ERROR) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Float, BackendError> getRate() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    return Outcome.fromResult(Utils.expRangeConvertInv(ttsRate, 0.1f, 1.0f, 6.0f));
  }

  @Override
  public Outcome<Unit, BackendError> setPitch(float pitch) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    ttsPitch = Utils.expRangeConvert(pitch, 0.25f, 1.0f, 4.0f);
    if (tts.setPitch(ttsPitch) == TextToSpeech.ERROR) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Float, BackendError> getPitch() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    return Outcome.fromResult(Utils.expRangeConvertInv(ttsPitch, 0.25f, 1.0f, 4.0f));
  }

  @Override
  public Outcome<Unit, BackendError> setVolume(float volume) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    ttsVolume = volume;
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Float, BackendError> getVolume() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    return Outcome.fromResult(ttsVolume);
  }

  @Override
  public Outcome<Unit, BackendError> refreshVoices() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    voiceList = buildVoiceList();
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Long, BackendError> countVoices() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    return Outcome.fromResult((long) voices().size());
  }

  @Override
  public Outcome<String, BackendError> getVoiceName(long id) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    final List<VoiceEntry> voices = voices();
    if (id < 0 || id >= voices.size()) return Outcome.fromError(BackendError.RANGE_OUT_OF_BOUNDS);
    return Outcome.fromResult(voices.get((int) id).name());
  }

  @Override
  public Outcome<String, BackendError> getVoiceLanguage(long id) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    final List<VoiceEntry> voices = voices();
    if (id < 0 || id >= voices.size()) return Outcome.fromError(BackendError.RANGE_OUT_OF_BOUNDS);
    return Outcome.fromResult(voices.get((int) id).voice().getLocale().toLanguageTag());
  }

  @Override
  public Outcome<Unit, BackendError> setVoice(long id) {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    final List<VoiceEntry> voices = voices();
    if (id < 0 || id >= voices.size()) return Outcome.fromError(BackendError.RANGE_OUT_OF_BOUNDS);
    final VoiceEntry entry = voices.get((int) id);
    if (!Objects.equals(entry.engine(), activeEngine)) return switchEngine(entry);
    if (tts.setVoice(entry.voice()) == TextToSpeech.ERROR) {
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    return Outcome.fromResult(new Unit());
  }

  @Override
  public Outcome<Long, BackendError> getVoice() {
    if (!isTTSInitialized) return Outcome.fromError(BackendError.NOT_INITIALIZED);
    final Voice current = tts.getVoice();
    if (current == null) return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    final List<VoiceEntry> voices = voices();
    final String engine = activeEngine;
    for (int index = 0; index < voices.size(); ++index) {
      final VoiceEntry entry = voices.get(index);
      if (Objects.equals(entry.engine(), engine)
          && entry.voice().getName().equals(current.getName())) {
        return Outcome.fromResult((long) index);
      }
    }
    return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
  }

  @Override
  public void close() {
    final TextToSpeech engine;
    synchronized (ttsStateLock) {
      engine = tts;
      tts = null;
      activeEngine = null;
      isTTSInitialized = false;
      voiceList = null;
      decoder = null;
    }
    shutdownEngine(engine);
    pendingUtterances.clear();
  }

  private List<VoiceEntry> voices() {
    if (voiceList == null) voiceList = buildVoiceList();
    return voiceList;
  }

  /**
   * Lists the voices of every installed engine: those of the engine in use first, then those of
   * each other engine in the order Android reports the engines, and the voices of each engine by
   * language and then by name. The engine in use answers over its existing connection. The other engines are all
   * connected at once and released once listed, so the list waits for one deadline rather than one
   * per engine; an engine that is not ready by then contributes no voices.
   */
  private List<VoiceEntry> buildVoiceList() {
    final List<VoiceEntry> voices = new ArrayList<>();
    final TextToSpeech current = tts;
    final String currentEngine = activeEngine;
    List<EngineInfo> engines;
    try {
      engines = current.getEngines();
    } catch (RuntimeException e) {
      engines = List.of();
    }
    addVoices(voices, currentEngine, engineLabel(engines, currentEngine), current);
    final var ctx = PrismContext.get();
    if (ctx == null) return voices;
    final List<EngineInfo> others = new ArrayList<>();
    final List<EngineConnection> connections = new ArrayList<>();
    for (final EngineInfo engine : engines) {
      if (engine.name.equals(currentEngine)) continue;
      others.add(engine);
      connections.add(new EngineConnection(ctx, engine.name));
    }
    final long deadline = initializationDeadline();
    for (int i = 0; i < connections.size(); ++i) {
      final EngineInfo engine = others.get(i);
      final TextToSpeech probe = connections.get(i).await(deadline);
      if (probe == null) continue;
      try {
        addVoices(voices, engine.name, engineLabel(engines, engine.name), probe);
      } finally {
        shutdownEngine(probe);
      }
    }
    return voices;
  }

  private static void addVoices(
      List<VoiceEntry> voices, String enginePackage, String label, TextToSpeech engine) {
    final Set<Voice> engineVoices;
    try {
      engineVoices = engine.getVoices();
    } catch (RuntimeException e) {
      return;
    }
    if (engineVoices == null) return;
    final List<Voice> sorted = new ArrayList<>(engineVoices);
    // Grouped by language, so a long list can be browsed engine, then language, then voice: each
    // name is "engine: language: voice", the language named in the phone's own language.
    sorted.sort(
        Comparator.comparing((Voice voice) -> languageName(voice)).thenComparing(Voice::getName));
    for (final Voice voice : sorted) {
      voices.add(
          new VoiceEntry(
              enginePackage, label + ": " + languageName(voice) + ": " + voice.getName(), voice));
    }
  }

  private static String languageName(Voice voice) {
    final Locale locale = voice.getLocale();
    if (locale == null) return "";
    final String name = locale.getDisplayName();
    return name.isEmpty() ? locale.toLanguageTag() : name;
  }

  private static String engineLabel(List<EngineInfo> engines, String enginePackage) {
    for (final EngineInfo engine : engines) {
      if (engine.name.equals(enginePackage) && engine.label != null && !engine.label.isEmpty()) {
        return engine.label;
      }
    }
    return enginePackage;
  }

  /**
   * Moves speech to the engine that provides {@code entry}. The new engine is connected and set up
   * before the old one is released, so a failure leaves the previous engine and voice in use.
   */
  private Outcome<Unit, BackendError> switchEngine(VoiceEntry entry) {
    final var ctx = PrismContext.get();
    if (ctx == null) return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    final TextToSpeech candidate =
        new EngineConnection(ctx, entry.engine()).await(initializationDeadline());
    if (candidate == null) return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    try {
      configureEngine(candidate, ttsRate, ttsPitch);
      if (candidate.setVoice(entry.voice()) == TextToSpeech.ERROR) {
        shutdownEngine(candidate);
        return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
      }
    } catch (RuntimeException e) {
      shutdownEngine(candidate);
      return Outcome.fromError(BackendError.INTERNAL_BACKEND_ERROR);
    }
    final TextToSpeech previous;
    synchronized (ttsStateLock) {
      previous = tts;
      tts = candidate;
      activeEngine = entry.engine();
    }
    shutdownEngine(previous);
    return Outcome.fromResult(new Unit());
  }

  private static long initializationDeadline() {
    return System.nanoTime() + TimeUnit.SECONDS.toNanos(INITIALIZATION_TIMEOUT_SECONDS);
  }

  private static void shutdownEngine(TextToSpeech engine) {
    if (engine == null) return;
    try {
      engine.stop();
    } catch (RuntimeException ignored) {
    }
    try {
      engine.shutdown();
    } catch (RuntimeException ignored) {
    }
  }

  /** A voice, the package name of the engine that provides it, and the name Prism reports. */
  private record VoiceEntry(String engine, String name, Voice voice) {}

  /**
   * One connection attempt to an engine. Android reports readiness asynchronously, and may report a
   * failure before the constructor returns, so the listener only records the status.
   */
  private static final class EngineConnection {
    private static final int PENDING = Integer.MIN_VALUE;
    private static final int ABANDONED = Integer.MIN_VALUE + 1;

    private final CountDownLatch initialized = new CountDownLatch(1);
    private final AtomicInteger status = new AtomicInteger(PENDING);
    private final TextToSpeech engine;

    /** Starts connecting to {@code enginePackage}, or to the preferred engine when it is null. */
    EngineConnection(Context ctx, String enginePackage) {
      TextToSpeech candidate;
      try {
        candidate =
            new TextToSpeech(
                ctx,
                result -> {
                  status.compareAndSet(PENDING, result);
                  initialized.countDown();
                },
                enginePackage);
      } catch (RuntimeException e) {
        candidate = null;
      }
      engine = candidate;
    }

    /**
     * Waits until {@code deadline}, a {@link System#nanoTime} value, for the engine to report
     * readiness. Returns the engine once it is ready; otherwise shuts it down and returns null.
     */
    TextToSpeech await(long deadline) {
      if (engine == null) return null;
      try {
        final long remaining = Math.max(0, deadline - System.nanoTime());
        if (!initialized.await(remaining, TimeUnit.NANOSECONDS)) {
          if (status.compareAndSet(PENDING, ABANDONED)) {
            shutdownEngine(engine);
            return null;
          }
        }
      } catch (InterruptedException e) {
        Thread.currentThread().interrupt();
        if (status.compareAndSet(PENDING, ABANDONED)) {
          shutdownEngine(engine);
          return null;
        }
      }
      if (status.get() != TextToSpeech.SUCCESS) {
        shutdownEngine(engine);
        return null;
      }
      return engine;
    }
  }
}
