// Freight Fate's Android activity: SDL's activity plus the few things the
// game asks of the platform (see crates/freight-fate/src/android.rs).
//
// The game itself is Rust in libmain.so; SDL starts it on its own thread
// and calls SDL_main. Every method below that the game calls runs on that
// thread or another game thread, never the UI thread, so none of them may
// touch views directly.

package net.orinks.freightfate;

import android.content.pm.PackageManager;
import android.content.res.AssetManager;
import android.content.res.Configuration;
import android.os.Bundle;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.util.Log;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;

import com.github.ethindp.prism.PrismContext;

import org.libsdl.app.SDLActivity;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;

public class FreightFateActivity extends SDLActivity {
    private static final String TAG = "FreightFate";

    /** The APK's copy of the game files, under assets/. */
    private static final String ASSET_ROOT = "game";

    /** The build's own stamp, which the game reads; copied with the rest. */
    private static final String BUILD_INFO = "build_info.json";

    /**
     * Which install the unpacked files came from, written last so an
     * interrupted unpack runs again. It names the APK's install time, not
     * just its build stamp: two test builds of one day share a stamp, and a
     * reinstall must still bring the new files.
     */
    private static final String UNPACKED = ".unpacked";

    /** What a TalkBack user hears when the game is in front with TalkBack on. */
    private static final String SUSPEND_TALKBACK =
            "Suspend TalkBack to play Freight Fate.";

    /** Long enough for TalkBack to finish announcing the game's window. */
    private static final long TALKBACK_REMINDER_DELAY_MS = 1500;

    @Override
    protected String[] getLibraries() {
        // Prism is C++; SDL is its own library (HIDAPI inside it) so its
        // JNI entry points stay visible (see the Cargo.toml note).
        return new String[] {"c++_shared", "SDL2", "main"};
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // Prism's Android voices need the application context before the
        // first one is built. The APK is assembled without merging Prism's
        // library manifest, so its init provider is not registered and the
        // context is handed over here instead.
        PrismContext.set(this);
        super.onCreate(savedInstanceState);
        AccessibilityManager manager = getSystemService(AccessibilityManager.class);
        if (manager != null) {
            manager.addTouchExplorationStateChangeListener(
                    enabled -> {
                        if (enabled) {
                            remindToSuspendTalkBack();
                        }
                    });
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (touchExplorationEnabled()) {
            remindToSuspendTalkBack();
        }
    }

    /**
     * Tell a TalkBack user, through TalkBack, that the game's gestures need
     * it suspended. TalkBack takes every touch for itself while it explores
     * by touch, and no app can let touches through, so the game's flicks
     * and holds cannot reach it until TalkBack is paused. Sent after
     * TalkBack has announced the window, so neither cuts the other off.
     */
    private void remindToSuspendTalkBack() {
        getWindow()
                .getDecorView()
                .postDelayed(
                        () -> {
                            AccessibilityManager manager =
                                    getSystemService(AccessibilityManager.class);
                            if (manager == null || !manager.isTouchExplorationEnabled()) {
                                return;
                            }
                            AccessibilityEvent event =
                                    new AccessibilityEvent(AccessibilityEvent.TYPE_ANNOUNCEMENT);
                            event.getText().add(SUSPEND_TALKBACK);
                            manager.sendAccessibilityEvent(event);
                        },
                        TALKBACK_REMINDER_DELAY_MS);
    }

    /**
     * Unpack the game files from the APK into the app's private storage,
     * unless this build's are already there. Returns the private files
     * folder (the game files are in its "game" folder), or null when the
     * unpacking failed.
     */
    public String prepareGameFiles() {
        File files = getFilesDir();
        File target = new File(files, ASSET_ROOT);
        try {
            AssetManager assets = getAssets();
            byte[] wanted = installStamp(assets);
            File stamp = new File(target, UNPACKED);
            if (stamp.isFile() && Arrays.equals(wanted, readFile(stamp))) {
                return files.getAbsolutePath();
            }
            Log.i(TAG, "Unpacking the game files");
            deleteTree(target);
            copyTree(assets, ASSET_ROOT, target);
            try (OutputStream out = new FileOutputStream(stamp)) {
                out.write(wanted);
            }
            return files.getAbsolutePath();
        } catch (IOException | PackageManager.NameNotFoundException e) {
            Log.e(TAG, "Could not unpack the game files", e);
            return null;
        }
    }

    /** Whether a screen reader is exploring by touch (TalkBack is on). */
    public boolean touchExplorationEnabled() {
        AccessibilityManager manager = getSystemService(AccessibilityManager.class);
        return manager != null && manager.isEnabled() && manager.isTouchExplorationEnabled();
    }

    /** Pixels per dp, which the game's gesture distances are measured in. */
    public float displayDensity() {
        return getResources().getDisplayMetrics().density;
    }

    /**
     * The phone's language named in itself, as Prism names each voice's
     * language, so the game can start a voice engine on a voice the player
     * understands.
     */
    public String defaultLanguageName() {
        return java.util.Locale.getDefault().getDisplayName();
    }

    /** Whether a hardware keyboard is connected and not hidden. */
    public boolean hardwareKeyboardConnected() {
        Configuration config = getResources().getConfiguration();
        return config.keyboard == Configuration.KEYBOARD_QWERTY
                && config.hardKeyboardHidden == Configuration.HARDKEYBOARDHIDDEN_NO;
    }

    /**
     * A gesture's haptic, matching the iPhone's: 0 a bump (a hold starting
     * or ending), 1 a tick (one step), 2 a warning (the deep swipe), 3 a
     * success (the engine key).
     */
    public void haptic(int kind) {
        Vibrator vibrator = getSystemService(Vibrator.class);
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        int effect;
        switch (kind) {
            case 0:
                effect = VibrationEffect.EFFECT_CLICK;
                break;
            case 2:
                effect = VibrationEffect.EFFECT_HEAVY_CLICK;
                break;
            case 3:
                effect = VibrationEffect.EFFECT_DOUBLE_CLICK;
                break;
            default:
                effect = VibrationEffect.EFFECT_TICK;
                break;
        }
        vibrator.vibrate(VibrationEffect.createPredefined(effect));
    }

    private PadRumble padRumble;

    /**
     * Rumble a controller SDL cannot rumble itself: both motors, 0 to 255,
     * for durationMs; 0 and 0 stops them. See {@link PadRumble}.
     */
    public synchronized void padRumble(int low, int high, int durationMs) {
        if (padRumble == null) {
            padRumble = new PadRumble(this);
        }
        padRumble.rumble(low, high, durationMs);
    }

    /** This install's build stamp and the time the APK was installed. */
    private byte[] installStamp(AssetManager assets)
            throws IOException, PackageManager.NameNotFoundException {
        long installed = getPackageManager().getPackageInfo(getPackageName(), 0).lastUpdateTime;
        byte[] info = readAsset(assets, ASSET_ROOT + "/" + BUILD_INFO);
        byte[] when = (" installed " + installed).getBytes(StandardCharsets.UTF_8);
        byte[] stamp = Arrays.copyOf(info, info.length + when.length);
        System.arraycopy(when, 0, stamp, info.length, when.length);
        return stamp;
    }

    private static void copyTree(AssetManager assets, String path, File target)
            throws IOException {
        String[] children = assets.list(path);
        if (children == null || children.length == 0) {
            copyFile(assets, path, target);
            return;
        }
        if (!target.isDirectory() && !target.mkdirs()) {
            throw new IOException("Could not create " + target);
        }
        for (String child : children) {
            copyTree(assets, path + "/" + child, new File(target, child));
        }
    }

    private static void copyFile(AssetManager assets, String path, File target) throws IOException {
        byte[] buffer = new byte[1 << 16];
        try (InputStream in = assets.open(path, AssetManager.ACCESS_STREAMING);
                OutputStream out = new FileOutputStream(target)) {
            int read;
            while ((read = in.read(buffer)) > 0) {
                out.write(buffer, 0, read);
            }
        }
    }

    private static byte[] readAsset(AssetManager assets, String path) throws IOException {
        try (InputStream in = assets.open(path)) {
            return in.readAllBytes();
        }
    }

    private static byte[] readFile(File file) {
        try (InputStream in = new java.io.FileInputStream(file)) {
            return in.readAllBytes();
        } catch (IOException e) {
            return new byte[0];
        }
    }

    private static void deleteTree(File file) {
        File[] children = file.listFiles();
        if (children != null) {
            for (File child : children) {
                deleteTree(child);
            }
        }
        file.delete();
    }
}
