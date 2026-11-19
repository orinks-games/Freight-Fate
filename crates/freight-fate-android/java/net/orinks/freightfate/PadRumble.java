package net.orinks.freightfate;

import android.content.Context;
import android.hardware.input.InputManager;
import android.os.CombinedVibration;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.SystemClock;
import android.os.VibrationEffect;
import android.os.VibratorManager;
import android.view.InputDevice;

import java.util.ArrayList;
import java.util.List;

/**
 * Controller rumble through Android, for a controller SDL cannot rumble.
 *
 * SDL 2.26 rumbles a controller only through its HIDAPI drivers, which on
 * Android reach a controller on a USB cable. A controller Android reads
 * itself, such as a PlayStation controller over Bluetooth, gets no rumble
 * from SDL, so the game asks this class instead. It plays on every
 * connected controller that has motors: Android gives the game no way to
 * match SDL's controller to its own, and the game uses one at a time.
 *
 * The game sends a level every frame while a rumble plays. Requests are
 * kept to the newest and played on their own thread, so a slow vibrator
 * service never holds up the game.
 */
final class PadRumble implements InputManager.InputDeviceListener {
    private final InputManager inputManager;
    private final Handler handler;
    private final Object lock = new Object();

    /** The controllers with motors; rebuilt when one comes or goes. */
    private List<VibratorManager> pads;

    /** The newest request, and whether a run to play it is posted. */
    private int low;
    private int high;
    private int durationMs;
    private boolean posted;

    /** What is playing now, so an unchanged level is not sent again. */
    private int playingLow;
    private int playingHigh;
    private long playingUntil;

    PadRumble(Context context) {
        inputManager = context.getSystemService(InputManager.class);
        HandlerThread thread = new HandlerThread("pad rumble");
        thread.start();
        handler = new Handler(thread.getLooper());
        if (inputManager != null) {
            inputManager.registerInputDeviceListener(this, handler);
        }
    }

    /** Both motors, 0 to 255, for durationMs; 0 and 0 stops them. */
    void rumble(int low, int high, int durationMs) {
        synchronized (lock) {
            this.low = low;
            this.high = high;
            this.durationMs = durationMs;
            if (posted) {
                return;
            }
            posted = true;
        }
        handler.post(this::play);
    }

    private void play() {
        int low;
        int high;
        int durationMs;
        synchronized (lock) {
            low = this.low;
            high = this.high;
            durationMs = this.durationMs;
            posted = false;
        }
        long now = SystemClock.uptimeMillis();
        boolean stop = low <= 0 && high <= 0 || durationMs <= 0;
        if (stop) {
            if (playingUntil > now) {
                for (VibratorManager pad : pads()) {
                    pad.cancel();
                }
            }
            playingUntil = 0;
            return;
        }
        // The same level with more than half its time left plays on.
        if (low == playingLow && high == playingHigh && playingUntil - now > durationMs / 2) {
            return;
        }
        for (VibratorManager pad : pads()) {
            pad.vibrate(vibration(pad.getVibratorIds(), low, high, durationMs));
        }
        playingLow = low;
        playingHigh = high;
        playingUntil = now + durationMs;
    }

    /**
     * The two levels on a controller's motors: the first motor (the
     * strong, left one) takes low, the second high. A controller with one
     * motor takes the stronger.
     */
    private static CombinedVibration vibration(int[] ids, int low, int high, int durationMs) {
        if (ids.length < 2) {
            return CombinedVibration.createParallel(oneShot(Math.max(low, high), durationMs));
        }
        CombinedVibration.ParallelCombination both = CombinedVibration.startParallel();
        if (low > 0) {
            both.addVibrator(ids[0], oneShot(low, durationMs));
        }
        if (high > 0) {
            both.addVibrator(ids[1], oneShot(high, durationMs));
        }
        return both.combine();
    }

    private static VibrationEffect oneShot(int level, int durationMs) {
        return VibrationEffect.createOneShot(durationMs, Math.min(255, Math.max(1, level)));
    }

    private List<VibratorManager> pads() {
        if (pads == null) {
            pads = findPads();
        }
        return pads;
    }

    private static List<VibratorManager> findPads() {
        List<VibratorManager> found = new ArrayList<>();
        for (int id : InputDevice.getDeviceIds()) {
            InputDevice device = InputDevice.getDevice(id);
            if (device == null || !isPad(device)) {
                continue;
            }
            VibratorManager manager = device.getVibratorManager();
            if (manager.getVibratorIds().length > 0) {
                found.add(manager);
            }
        }
        return found;
    }

    private static boolean isPad(InputDevice device) {
        int sources = device.getSources();
        return (sources & InputDevice.SOURCE_GAMEPAD) == InputDevice.SOURCE_GAMEPAD
                || (sources & InputDevice.SOURCE_JOYSTICK) == InputDevice.SOURCE_JOYSTICK;
    }

    @Override
    public void onInputDeviceAdded(int deviceId) {
        pads = null;
    }

    @Override
    public void onInputDeviceRemoved(int deviceId) {
        pads = null;
    }

    @Override
    public void onInputDeviceChanged(int deviceId) {
        pads = null;
    }
}
