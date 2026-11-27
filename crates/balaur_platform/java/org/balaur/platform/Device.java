package org.balaur.platform;

import android.app.Activity;
import android.app.NotificationManager;
import android.content.ActivityNotFoundException;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.os.VibratorManager;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * What a phone lends a game: its motor, the share sheet, and whether each
 * service works here ({@code platform.is_available}).
 *
 * {@code balaur_platform} calls these from the game's thread. The share
 * sheet answers through {@link #answer}, which the same crate registers, as
 * the JSON map a script's waiter receives.
 */
public final class Device {
    // The words balaur_platform's vocabulary spells.
    private static final String HAPTICS = "haptics";
    private static final String SHARE = "share";
    private static final String PHOTO_PICKER = "photo_picker";
    private static final String NOTIFICATIONS = "notifications";
    private static final String WEB_SIGN_IN = "web_sign_in";
    private static final String LIGHT = "light";
    private static final String MEDIUM = "medium";
    private static final String HEAVY = "heavy";
    private static final String SELECTION = "selection";
    private static final String SUCCESS = "success";
    private static final String WARNING = "warning";

    static final String KIND = "kind";
    static final String ERROR = "error";

    private static final int PREDEFINED_EFFECTS_API = 29;
    private static final int VIBRATOR_MANAGER_API = 31;

    private Device() {}

    /** A request's answer, as JSON. Registered by balaur_platform. */
    static native void answer(long request, String json);

    /** Whether {@code service} works on this device. */
    public static boolean available(Activity game, String service) {
        switch (service) {
            case HAPTICS:
                Vibrator vibrator = vibrator(game);
                return vibrator != null && vibrator.hasVibrator();
            case SHARE:
            case PHOTO_PICKER:
                return true;
            case NOTIFICATIONS:
                return game.getSystemService(NotificationManager.class) != null;
            case WEB_SIGN_IN:
                return game.getPackageManager().resolveActivity(
                        new Intent(Intent.ACTION_VIEW, Uri.parse("https://")), 0) != null;
            default:
                return false;
        }
    }

    /** Buzz for {@code milliseconds}. */
    public static void vibrate(Activity game, long milliseconds) {
        if (milliseconds > 0) {
            play(game, VibrationEffect.createOneShot(milliseconds, VibrationEffect.DEFAULT_AMPLITUDE));
        }
    }

    /** One tap of feedback: a predefined effect from Android 10, a short buzz before. */
    public static void haptic(Activity game, String effect) {
        play(game, effect(effect));
    }

    /** The system's chooser over the game; {@link Share} answers once it shuts. */
    public static void share(final Activity game, final long request, final String text, final String file,
            final String title) {
        game.runOnUiThread(() -> {
            try {
                game.startActivity(Share.intent(game, request, text, file, title));
            } catch (ActivityNotFoundException e) {
                answer(request, failed("the template's manifest has no Share activity").toString());
            }
        });
    }

    /** The system's photo picker over the game; {@link PickPhoto} answers once it shuts. */
    public static void pickPhoto(final Activity game, final long request) {
        game.runOnUiThread(() -> {
            try {
                game.startActivity(PickPhoto.intent(game, request));
            } catch (ActivityNotFoundException e) {
                answer(request, failed("the template's manifest has no PickPhoto activity").toString());
            }
        });
    }

    private static VibrationEffect effect(String name) {
        boolean predefined = Build.VERSION.SDK_INT >= PREDEFINED_EFFECTS_API;
        switch (name) {
            case LIGHT:
            case SELECTION:
                return predefined ? VibrationEffect.createPredefined(VibrationEffect.EFFECT_TICK) : buzz(10);
            case MEDIUM:
                return predefined ? VibrationEffect.createPredefined(VibrationEffect.EFFECT_CLICK) : buzz(20);
            case HEAVY:
                return predefined ? VibrationEffect.createPredefined(VibrationEffect.EFFECT_HEAVY_CLICK) : buzz(40);
            case SUCCESS:
                return predefined
                        ? VibrationEffect.createPredefined(VibrationEffect.EFFECT_DOUBLE_CLICK)
                        : VibrationEffect.createWaveform(new long[] {0, 20, 60, 20}, -1);
            case WARNING:
                return VibrationEffect.createWaveform(new long[] {0, 30, 80, 30}, -1);
            default:
                // An error, as iOS taps it: three.
                return VibrationEffect.createWaveform(new long[] {0, 30, 60, 30, 60, 30}, -1);
        }
    }

    private static VibrationEffect buzz(long milliseconds) {
        return VibrationEffect.createOneShot(milliseconds, VibrationEffect.DEFAULT_AMPLITUDE);
    }

    @SuppressWarnings("deprecation") // VIBRATOR_SERVICE is the only way below API 31
    private static Vibrator vibrator(Context context) {
        if (Build.VERSION.SDK_INT >= VIBRATOR_MANAGER_API) {
            VibratorManager manager = context.getSystemService(VibratorManager.class);
            return manager == null ? null : manager.getDefaultVibrator();
        }
        return (Vibrator) context.getSystemService(Context.VIBRATOR_SERVICE);
    }

    private static void play(Context context, VibrationEffect effect) {
        Vibrator vibrator = vibrator(context);
        if (vibrator == null || !vibrator.hasVibrator()) {
            return;
        }
        try {
            vibrator.vibrate(effect);
        } catch (SecurityException e) {
            // A manifest without VIBRATE: the game keeps running, unbuzzed.
        }
    }

    static JSONObject kind(String kind) {
        JSONObject out = new JSONObject();
        try {
            out.put(KIND, kind);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    static JSONObject failed(String message) {
        JSONObject out = kind(ERROR);
        try {
            out.put(ERROR, message == null ? "" : message);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }
}
