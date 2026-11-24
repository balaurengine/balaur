package org.balaur.google;

import android.app.Activity;
import android.app.Application;
import android.content.Intent;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;

import com.google.android.gms.common.ConnectionResult;
import com.google.android.gms.common.GoogleApiAvailabilityLight;
import com.google.firebase.FirebaseApp;
import com.google.firebase.messaging.FirebaseMessaging;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONTokener;

/**
 * Firebase Cloud Messaging behind {@code platform.register_for_push}: the
 * device's registration token, and the pushes that reach the game.
 *
 * {@code balaur_platform} calls {@link #start} from the game's thread and
 * registers {@link #answer}. {@code listen} hands over what arrived before
 * the game did: a token {@link Messaging} heard, a push received in front,
 * and the tap that launched the game, read off the activity's intent. Every
 * static here is touched on the UI thread alone.
 */
public final class Push {
    // The words balaur_platform's `kind` module and keys spell.
    private static final String KIND = "kind";
    private static final String ERROR = "error";
    private static final String REGISTERED = "push_registered";
    private static final String FAILED = "push_failed";
    private static final String TOKEN_CHANGED = "push_token_changed";
    private static final String RECEIVED = "push_received";
    private static final String OPENED = "notification_opened";
    private static final String KEY_TOKEN = "token";
    private static final String KEY_PROVIDER = "provider";
    private static final String KEY_PLATFORM = "platform";
    private static final String KEY_ID = "id";
    private static final String KEY_TITLE = "title";
    private static final String KEY_BODY = "body";
    private static final String KEY_DATA = "data";
    // Gamend's push_tokens values.
    private static final String FCM = "fcm";
    private static final String ANDROID = "android";

    private static final String REGISTER = "register_for_push";
    private static final String LISTEN = "listen";

    // What FCM adds to a launch intent beside the message's own data.
    private static final String MESSAGE_ID = "google.message_id";
    private static final String[] RESERVED_PREFIXES = {"google.", "gcm."};
    private static final String[] RESERVED_KEYS = {"from", "collapse_key"};

    /** What arrives unasked carries the request no call is given. */
    private static final long ARRIVAL = 0;

    /** Whether the game takes arrivals yet. */
    private static boolean listening;
    /** What arrived before it did. */
    private static final List<String> waiting = new ArrayList<>();
    /** The taps already handed over, by message id: a resumed activity keeps its intent. */
    private static final Set<String> opened = new HashSet<>();
    /** The token register_for_push answered last, which onNewToken repeats on a first launch. */
    private static String answered;

    private Push() {}

    /** A request's answer, or under request 0 an arrival, as JSON. Registered by balaur_platform. */
    static native void answer(long request, String json);

    /** Whether Firebase started from the export's string resources, and Play services is here. */
    public static boolean available(Activity game) {
        return !FirebaseApp.getApps(game).isEmpty()
                && GoogleApiAvailabilityLight.getInstance().isGooglePlayServicesAvailable(game)
                        == ConnectionResult.SUCCESS;
    }

    /** {@code register_for_push} for {@code request}, or {@code listen} under request 0. */
    public static void start(final Activity game, final long request, final String call, final String args) {
        game.runOnUiThread(() -> {
            if (LISTEN.equals(call)) {
                listen(game);
            } else if (REGISTER.equals(call)) {
                register(request);
            } else {
                answer(request, failed(call + ": not a push call").toString());
            }
        });
    }

    // getToken() is FCM's default path; Messaging.onNewToken says why it is deprecated.
    @SuppressWarnings("deprecation")
    private static void register(final long request) {
        try {
            FirebaseMessaging.getInstance().getToken().addOnCompleteListener(task -> {
                if (task.isSuccessful() && task.getResult() != null) {
                    answered = task.getResult();
                    answer(request, token(REGISTERED, answered).toString());
                } else {
                    answer(request, failed(String.valueOf(task.getException())).toString());
                }
            });
        } catch (RuntimeException e) {
            // FirebaseApp did not start: the export wrote no google_app_id.
            answer(request, failed(e.toString()).toString());
        }
    }

    private static void listen(Activity game) {
        if (listening) {
            return;
        }
        listening = true;
        for (String json : waiting) {
            answer(ARRIVAL, json);
        }
        waiting.clear();
        tapped(game.getIntent());
        // NativeActivity forwards no new intent: a tap while the game is in
        // the background recreates it with FCM's intent, which this reads.
        game.getApplication().registerActivityLifecycleCallbacks(new Taps());
    }

    /** FCM replaced the token. */
    static void tokenChanged(final String token) {
        onUi(() -> {
            if (!token.equals(answered)) {
                arrive(token(TOKEN_CHANGED, token).toString());
            }
        });
    }

    /** A message reached the app while it ran. */
    static void received(final String id, final String title, final String body, final Map<String, String> data) {
        final JSONObject decoded = decoded(data);
        onUi(() -> {
            JSONObject arrival = kind(RECEIVED);
            put(arrival, KEY_ID, id);
            put(arrival, KEY_TITLE, title);
            put(arrival, KEY_BODY, body);
            put(arrival, KEY_DATA, decoded);
            arrive(arrival.toString());
        });
    }

    /** An activity started by a tap on a notification FCM showed carries the message in its extras. */
    static void tapped(Intent intent) {
        Bundle extras = intent == null ? null : intent.getExtras();
        if (extras == null || extras.getString(MESSAGE_ID) == null) {
            return;
        }
        String id = extras.getString(MESSAGE_ID);
        if (!opened.add(id)) {
            return;
        }
        Map<String, String> data = new HashMap<>();
        for (String key : extras.keySet()) {
            // FCM's own extras, a long among them, are skipped before reading one as a string.
            String value = reserved(key) ? null : extras.getString(key);
            if (value != null) {
                data.put(key, value);
            }
        }
        JSONObject arrival = kind(OPENED);
        put(arrival, KEY_ID, id);
        put(arrival, KEY_DATA, decoded(data));
        arrive(arrival.toString());
    }

    private static boolean reserved(String key) {
        for (String prefix : RESERVED_PREFIXES) {
            if (key.startsWith(prefix)) {
                return true;
            }
        }
        for (String name : RESERVED_KEYS) {
            if (name.equals(key)) {
                return true;
            }
        }
        return false;
    }

    /**
     * FCM's data is strings, and Gamend writes a value that is not one as its
     * JSON: each value that reads whole as JSON other than a string is that
     * value again.
     */
    static JSONObject decoded(Map<String, String> data) {
        JSONObject out = new JSONObject();
        if (data == null) {
            return out;
        }
        for (Map.Entry<String, String> entry : data.entrySet()) {
            put(out, entry.getKey(), decodedValue(entry.getValue()));
        }
        return out;
    }

    private static Object decodedValue(String text) {
        if (text == null) {
            return JSONObject.NULL;
        }
        try {
            JSONTokener tokener = new JSONTokener(text);
            Object value = tokener.nextValue();
            if (tokener.nextClean() == 0 && !(value instanceof String)) {
                return value;
            }
        } catch (JSONException e) {
            // Not JSON: the string itself.
        }
        return text;
    }

    private static JSONObject token(String kind, String token) {
        JSONObject out = kind(kind);
        put(out, KEY_TOKEN, token);
        put(out, KEY_PROVIDER, FCM);
        put(out, KEY_PLATFORM, ANDROID);
        return out;
    }

    private static JSONObject failed(String message) {
        JSONObject out = kind(FAILED);
        put(out, ERROR, message);
        return out;
    }

    private static JSONObject kind(String kind) {
        JSONObject out = new JSONObject();
        put(out, KIND, kind);
        return out;
    }

    private static void put(JSONObject out, String key, Object value) {
        try {
            out.put(key, value == null ? "" : value);
        } catch (JSONException e) {
            // A string key and a JSON value never fail.
        }
    }

    private static void arrive(String json) {
        if (listening) {
            answer(ARRIVAL, json);
        } else {
            waiting.add(json);
        }
    }

    private static void onUi(Runnable body) {
        new Handler(Looper.getMainLooper()).post(body);
    }

    /** Reads the intent of each activity that resumes: the recreated game a tap started. */
    private static final class Taps implements Application.ActivityLifecycleCallbacks {
        @Override
        public void onActivityResumed(Activity activity) {
            tapped(activity.getIntent());
        }

        @Override
        public void onActivityCreated(Activity activity, Bundle saved) {}

        @Override
        public void onActivityStarted(Activity activity) {}

        @Override
        public void onActivityPaused(Activity activity) {}

        @Override
        public void onActivityStopped(Activity activity) {}

        @Override
        public void onActivitySaveInstanceState(Activity activity, Bundle out) {}

        @Override
        public void onActivityDestroyed(Activity activity) {}
    }
}
