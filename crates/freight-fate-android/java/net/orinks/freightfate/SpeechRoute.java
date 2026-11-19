// Keeps the game's speech out of a game controller's haptic motors.
//
// A PlayStation controller on a USB cable is also a USB audio device, and
// Android sends the phone's sound to it, to the headset plugged into the
// controller. A DualSense's USB audio has four channels: the first two are
// the headset or speaker, the last two drive its haptic motors. Android
// spreads a one-channel voice across all four, so every spoken line buzzes
// in the player's hands. TextToSpeech.speak() gives no say in that, so while
// such a controller is connected the speech backend synthesizes each line
// here instead and plays it on an AudioTrack of its own on the controller,
// with the voice in the first two channels and silence in the rest. With no
// controller audio connected the backend speaks as before.
//
// What each connected output offers, and where each line actually played,
// is written to logs/speech-route.log in the game's storage when it changes.
package net.orinks.freightfate;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioDeviceInfo;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.speech.tts.TextToSpeech;
import android.system.ErrnoException;
import android.system.Os;
import android.util.Log;
import java.io.File;
import java.io.FileDescriptor;
import java.io.FileWriter;
import java.io.IOException;
import java.text.SimpleDateFormat;
import java.util.Arrays;
import java.util.Date;
import java.util.Locale;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.LinkedBlockingDeque;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.Supplier;

public final class SpeechRoute {
  private static final String TAG = "FreightFate";
  // Longest a line may take to synthesize before it is given up.
  private static final long SYNTHESIS_TIMEOUT_MS = 60_000;
  // The log is a diagnostic; it starts over past this size.
  private static final long LOG_LIMIT = 256 * 1024;

  private final AudioManager audio;
  private final AudioAttributes attributes;
  private final Supplier<TextToSpeech> engine;
  private final File logFile;
  private final LinkedBlockingDeque<Line> queue = new LinkedBlockingDeque<>();
  private final ConcurrentHashMap<String, Utterance> utterances = new ConcurrentHashMap<>();
  // Bumped by cancel(); a line from an older generation is dropped.
  private final AtomicLong generation = new AtomicLong();
  private volatile Utterance current;
  private volatile AudioTrack playing;
  private volatile String lastOutputs = "";
  private volatile String lastPlayed = "";

  /** Where a line plays: the controller, with this many channels in each frame. */
  public record Target(AudioDeviceInfo device, int channels) {}

  private record Line(String text, float volume, Target target, long generation) {}

  private record Begin(int sampleRate, int encoding, int channels) {}

  private static final Object END = new Object();

  // One line's synthesis events, from the engine's callbacks to the player.
  private static final class Utterance {
    final LinkedBlockingQueue<Object> events = new LinkedBlockingQueue<>();
  }

  public SpeechRoute(
      Context context, AudioAttributes attributes, Supplier<TextToSpeech> engine) {
    this.audio = context.getSystemService(AudioManager.class);
    this.attributes = attributes;
    this.engine = engine;
    this.logFile = new File(context.getFilesDir(), "FreightFate/logs/speech-route.log");
    Thread worker = new Thread(this::run, "speech route");
    worker.setDaemon(true);
    worker.start();
  }

  /**
   * The connected controller's audio output, or null while there is none (Android chooses, as
   * always).
   */
  public Target pickOutput() {
    if (audio == null) return null;
    AudioDeviceInfo[] outputs = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS);
    logOutputs(outputs);
    for (AudioDeviceInfo device : outputs) {
      if (isController(device)) {
        int[] counts = device.getChannelCounts();
        // Four channels when the controller takes four (or does not say), so the last two,
        // the motors, are written as silence rather than left to Android's upmix.
        boolean four = counts.length == 0 || Arrays.stream(counts).anyMatch(c -> c >= 4);
        return new Target(device, four ? 4 : 2);
      }
    }
    return null;
  }

  /** Whether a line is playing or waiting. */
  public boolean busy() {
    return current != null || !queue.isEmpty();
  }

  /** Queue `text` to play on `target`, after anything already queued. */
  public void enqueue(String text, float volume, Target target) {
    queue.add(new Line(text, volume, target, generation.get()));
  }

  /** Drop every queued line and cut the one playing. The caller stops the engine. */
  public void cancel() {
    generation.incrementAndGet();
    queue.clear();
    AudioTrack track = playing;
    if (track != null) {
      try {
        track.pause();
        track.flush();
      } catch (IllegalStateException ignored) {
      }
    }
  }

  // The engine's progress callbacks, for the utterances this class started. Each returns
  // whether the utterance was one of them.

  public boolean onBegin(String id, int sampleRate, int encoding, int channels) {
    Utterance utterance = utterances.get(id);
    if (utterance == null) return false;
    utterance.events.add(new Begin(sampleRate, encoding, channels));
    return true;
  }

  public boolean onAudio(String id, byte[] pcm) {
    Utterance utterance = utterances.get(id);
    if (utterance == null) return false;
    utterance.events.add(pcm);
    return true;
  }

  public boolean onEnd(String id) {
    Utterance utterance = utterances.get(id);
    if (utterance == null) return false;
    utterance.events.add(END);
    return true;
  }

  private void run() {
    while (true) {
      Line line;
      try {
        line = queue.take();
      } catch (InterruptedException e) {
        return;
      }
      try {
        play(line);
      } catch (RuntimeException e) {
        Log.w(TAG, "speech route: a line failed", e);
        log("a line failed: " + e);
      }
    }
  }

  private boolean stale(Line line) {
    return line.generation() != generation.get();
  }

  private void play(Line line) {
    if (stale(line)) return;
    TextToSpeech tts = engine.get();
    if (tts == null) return;
    String id = "route-" + UUID.randomUUID();
    Utterance utterance = new Utterance();
    utterances.put(id, utterance);
    current = utterance;
    ParcelFileDescriptor sink = null;
    AudioTrack track = null;
    try {
      // The engine writes the line to a file as well; it is only memory, and is thrown away.
      FileDescriptor memory = Os.memfd_create("speech_route", 0);
      sink = ParcelFileDescriptor.dup(memory);
      Os.close(memory);
      if (tts.synthesizeToFile(line.text(), new Bundle(), sink, id) != TextToSpeech.SUCCESS) {
        return;
      }
      long deadline = System.currentTimeMillis() + SYNTHESIS_TIMEOUT_MS;
      Spreader spreader = null;
      long frames = 0;
      int sampleRate = 0;
      boolean reported = false;
      while (!stale(line) && System.currentTimeMillis() < deadline) {
        Object event = utterance.events.poll(50, TimeUnit.MILLISECONDS);
        if (event == null) continue;
        if (event == END) break;
        if (event instanceof Begin begin) {
          if (track != null) continue;
          int sample = bytesPerSample(begin.encoding());
          spreader = new Spreader(begin.channels(), line.target().channels(), sample);
          track = open(begin, line);
          if (track == null) return;
          playing = track;
          sampleRate = begin.sampleRate();
        } else if (event instanceof byte[] pcm && track != null) {
          byte[] out = spreader.spread(pcm);
          if (!write(track, out, line)) break;
          frames += spreader.framesIn(out);
          if (!reported) {
            reported = true;
            logPlayed(track, line, spreader);
          }
        }
      }
      if (track != null && !stale(line)) {
        track.stop(); // plays out what was written, then stops
        long left = sampleRate > 0 ? frames * 1000 / sampleRate : 0;
        long until = System.currentTimeMillis() + left + 500;
        while (!stale(line)
            && System.currentTimeMillis() < until
            && (track.getPlaybackHeadPosition() & 0xffffffffL) < frames) {
          Thread.sleep(10);
        }
      }
    } catch (ErrnoException | IOException e) {
      log("no memory file for a line: " + e);
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
    } finally {
      utterances.remove(id);
      current = null;
      playing = null;
      if (track != null) track.release();
      if (sink != null) {
        try {
          sink.close();
        } catch (IOException ignored) {
        }
      }
    }
  }

  private AudioTrack open(Begin begin, Line line) {
    int mask =
        line.target().channels() == 4
            ? AudioFormat.CHANNEL_OUT_QUAD
            : AudioFormat.CHANNEL_OUT_STEREO;
    AudioFormat format =
        new AudioFormat.Builder()
            .setEncoding(begin.encoding())
            .setSampleRate(begin.sampleRate())
            .setChannelMask(mask)
            .build();
    int minimum = AudioTrack.getMinBufferSize(begin.sampleRate(), mask, begin.encoding());
    try {
      AudioTrack track =
          new AudioTrack.Builder()
              .setAudioAttributes(attributes)
              .setAudioFormat(format)
              .setTransferMode(AudioTrack.MODE_STREAM)
              .setBufferSizeInBytes(Math.max(minimum, 8192) * 2)
              .build();
      track.setPreferredDevice(line.target().device());
      track.setVolume(line.volume());
      track.play();
      return track;
    } catch (IllegalArgumentException | UnsupportedOperationException | IllegalStateException e) {
      log("no audio track for " + begin + " on " + line.target().channels() + " channels: " + e);
      return null;
    }
  }

  // Writes without blocking, so a cancel is noticed between pieces. False once cancelled.
  private boolean write(AudioTrack track, byte[] pcm, Line line) throws InterruptedException {
    int offset = 0;
    while (offset < pcm.length) {
      if (stale(line)) return false;
      int wrote = track.write(pcm, offset, pcm.length - offset, AudioTrack.WRITE_NON_BLOCKING);
      if (wrote < 0) return false;
      offset += wrote;
      if (wrote == 0) Thread.sleep(5);
    }
    return true;
  }

  // Turns the engine's frames (one or two channels) into the track's: the voice in the first
  // two channels (a one-channel voice in both), silence in the rest. A chunk can end partway
  // through a frame, so the remainder waits for the next chunk.
  private static final class Spreader {
    private final int in;
    private final int out;
    private final int sample;
    private byte[] carry = new byte[0];

    Spreader(int in, int out, int sample) {
      this.in = Math.max(1, in);
      this.out = out;
      this.sample = sample;
    }

    byte[] spread(byte[] pcm) {
      byte[] data = pcm;
      if (carry.length > 0) {
        data = new byte[carry.length + pcm.length];
        System.arraycopy(carry, 0, data, 0, carry.length);
        System.arraycopy(pcm, 0, data, carry.length, pcm.length);
      }
      int inFrame = in * sample;
      int frames = data.length / inFrame;
      carry = Arrays.copyOfRange(data, frames * inFrame, data.length);
      byte[] result = new byte[frames * out * sample];
      for (int frame = 0; frame < frames; frame++) {
        int from = frame * inFrame;
        int to = frame * out * sample;
        System.arraycopy(data, from, result, to, sample);
        int right = in >= 2 ? from + sample : from;
        System.arraycopy(data, right, result, to + sample, sample);
      }
      return result;
    }

    long framesIn(byte[] spread) {
      return spread.length / (out * sample);
    }

    @Override
    public String toString() {
      return in + " channel(s) into " + out;
    }
  }

  private static int bytesPerSample(int encoding) {
    return switch (encoding) {
      case AudioFormat.ENCODING_PCM_8BIT -> 1;
      case AudioFormat.ENCODING_PCM_FLOAT, AudioFormat.ENCODING_PCM_32BIT -> 4;
      case AudioFormat.ENCODING_PCM_24BIT_PACKED -> 3;
      default -> 2;
    };
  }

  // A game controller's own audio: a USB audio device named as a controller (Sony's DualSense
  // and DualShock 4 call themselves "Wireless Controller").
  private static boolean isController(AudioDeviceInfo device) {
    int type = device.getType();
    if (type != AudioDeviceInfo.TYPE_USB_HEADSET
        && type != AudioDeviceInfo.TYPE_USB_DEVICE
        && type != AudioDeviceInfo.TYPE_USB_ACCESSORY) {
      return false;
    }
    CharSequence product = device.getProductName();
    if (product == null) return false;
    String name = product.toString().toLowerCase(Locale.ROOT);
    return name.contains("controller") || name.contains("dualsense") || name.contains("dualshock");
  }

  private static String describe(AudioDeviceInfo device) {
    return "type "
        + device.getType()
        + " \""
        + device.getProductName()
        + "\" channel counts "
        + Arrays.toString(device.getChannelCounts())
        + " masks "
        + Arrays.toString(device.getChannelMasks())
        + " index masks "
        + Arrays.toString(device.getChannelIndexMasks())
        + " encodings "
        + Arrays.toString(device.getEncodings())
        + " rates "
        + Arrays.toString(device.getSampleRates());
  }

  private void logOutputs(AudioDeviceInfo[] outputs) {
    StringBuilder text = new StringBuilder();
    for (AudioDeviceInfo device : outputs) {
      text.append("\n  ").append(describe(device));
    }
    String now = text.toString();
    if (!now.equals(lastOutputs)) {
      lastOutputs = now;
      log("outputs:" + now);
    }
  }

  private void logPlayed(AudioTrack track, Line line, Spreader spreader) {
    AudioDeviceInfo routed = track.getRoutedDevice();
    String now =
        "speech plays on "
            + (routed == null ? "an unknown output" : describe(routed))
            + ", "
            + spreader
            + " (asked for \""
            + line.target().device().getProductName()
            + "\")";
    if (!now.equals(lastPlayed)) {
      lastPlayed = now;
      log(now);
    }
  }

  private synchronized void log(String text) {
    Log.i(TAG, "speech route: " + text);
    try {
      File folder = logFile.getParentFile();
      if (folder != null) folder.mkdirs();
      boolean append = logFile.length() < LOG_LIMIT;
      try (FileWriter writer = new FileWriter(logFile, append)) {
        String time = new SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.ROOT).format(new Date());
        writer.write(time + " " + text + "\n");
      }
    } catch (IOException ignored) {
    }
  }
}
