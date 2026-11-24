package org.balaur.platform;

import android.Manifest;
import android.app.Activity;
import android.app.AlarmManager;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.ActivityNotFoundException;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;

import java.util.ArrayList;
import java.util.List;

import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONTokener;

/**
 * Local notifications: {@code platform.request_notifications}, {@code notify}
 * and {@code cancel_notification} on Android.
 *
 * {@code AlarmManager} holds the schedule and {@link Alarm} posts each one
 * on the game's own channel. A tap opens {@link Opened}, which brings the
 * game forward; what the notification carried waits here until the game
 * {@link #listen}s, so a tap that started the process arrives too.
 *
 * {@code balaur_platform} calls these from the game's thread and registers
 * {@link #answer}. Every static here is touched on the UI thread alone.
 */
public final class Notifications {
    static final String CHANNEL = "balaur";
    static final String ID = "org.balaur.platform.id";
    static final String TITLE = "org.balaur.platform.title";
    static final String BODY = "org.balaur.platform.body";
    static final String DATA = "org.balaur.platform.data";
    static final String REQUEST = "org.balaur.platform.request";
    private static final String SCHEME = "balaur-notification";
    private static final int POST_NOTIFICATIONS_API = 33;
    /** The one notification a tag holds: the tag is the game's id. */
    static final int ONE = 0;

    private static final String NOTIFICATIONS = "notifications";
    private static final String SCHEDULED = "scheduled";
    private static final String OPENED = "notification_opened";
    private static final String RECEIVED = "notification_received";
    private static final String ALLOWED = "allowed";
    private static final String KEY_ID = "id";
    private static final String KEY_TITLE = "title";
    private static final String KEY_BODY = "body";
    private static final String KEY_DATA = "data";

    /** Whether the game takes arrivals yet. */
    private static boolean listening;
    /** What arrived before it did. */
    private static final List<String> waiting = new ArrayList<>();

    private Notifications() {}

    /** A request's answer as JSON, or under request 0 an arrival. Registered by balaur_platform. */
    static native void answer(long request, String json);

    /** What arrives unasked carries the request no call is given. */
    private static final long ARRIVAL = 0;

    /** Ask for POST_NOTIFICATIONS from Android 13, else say whether the player left them on. */
    public static void request(final Activity game, final long request) {
        game.runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT < POST_NOTIFICATIONS_API
                    || game.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                            == PackageManager.PERMISSION_GRANTED) {
                allowed(request, manager(game).areNotificationsEnabled());
                return;
            }
            try {
                game.startActivity(new Intent(game, Permission.class).putExtra(REQUEST, request));
            } catch (ActivityNotFoundException e) {
                answer(request, Device.failed("the template's manifest has no Permission activity").toString());
            }
        });
    }

    /** Post {@code body} {@code delay} milliseconds from now, carrying {@code data} back on a tap. */
    public static void schedule(final Activity game, final long request, final String id, final String title,
            final String body, final long delay, final String data) {
        game.runOnUiThread(() -> {
            Context context = game.getApplicationContext();
            Intent alarm = alarm(context, id)
                    .putExtra(ID, id)
                    .putExtra(TITLE, title)
                    .putExtra(BODY, body)
                    .putExtra(DATA, data);
            if (delay <= 0) {
                Alarm.post(context, alarm);
            } else {
                // Inexact: an exact alarm needs SCHEDULE_EXACT_ALARM, which Play reviews.
                context.getSystemService(AlarmManager.class).setAndAllowWhileIdle(
                        AlarmManager.RTC_WAKEUP, System.currentTimeMillis() + delay, pending(context, alarm));
            }
            JSONObject answer = Device.kind(SCHEDULED);
            put(answer, KEY_ID, id);
            answer(request, answer.toString());
        });
    }

    /** Take {@code id} off the schedule and out of the shade. */
    public static void cancel(final Activity game, final String id) {
        game.runOnUiThread(() -> {
            Context context = game.getApplicationContext();
            PendingIntent pending = PendingIntent.getBroadcast(context, 0, alarm(context, id),
                    PendingIntent.FLAG_NO_CREATE | PendingIntent.FLAG_IMMUTABLE);
            if (pending != null) {
                context.getSystemService(AlarmManager.class).cancel(pending);
                pending.cancel();
            }
            manager(context).cancel(id, ONE);
        });
    }

    /** Hand over what arrived, and everything that arrives from now on. */
    public static void listen(final Activity game) {
        game.runOnUiThread(() -> {
            listening = true;
            for (String json : waiting) {
                answer(ARRIVAL, json);
            }
            waiting.clear();
        });
    }

    static void allowed(long request, boolean allowed) {
        JSONObject answer = Device.kind(NOTIFICATIONS);
        try {
            answer.put(ALLOWED, allowed);
        } catch (JSONException e) {
            // A string key and a boolean never fail.
        }
        answer(request, answer.toString());
    }

    /** A notification was tapped. */
    static void opened(String id, String data) {
        JSONObject arrival = Device.kind(OPENED);
        put(arrival, KEY_ID, id);
        put(arrival, KEY_DATA, parsed(data));
        arrive(arrival.toString());
    }

    /** A notification was shown while the game was in front. */
    static void received(String id, String title, String body, String data) {
        JSONObject arrival = Device.kind(RECEIVED);
        put(arrival, KEY_ID, id);
        put(arrival, KEY_TITLE, title);
        put(arrival, KEY_BODY, body);
        put(arrival, KEY_DATA, parsed(data));
        arrive(arrival.toString());
    }

    static NotificationManager manager(Context context) {
        return context.getSystemService(NotificationManager.class);
    }

    /** The game's one channel, named after it. Creating it again changes nothing. */
    static void channel(Context context) {
        CharSequence label = context.getApplicationInfo().loadLabel(context.getPackageManager());
        manager(context).createNotificationChannel(
                new NotificationChannel(CHANNEL, label, NotificationManager.IMPORTANCE_DEFAULT));
    }

    /** The alarm for {@code id}: its data URI tells two apart, as extras do not. */
    private static Intent alarm(Context context, String id) {
        return new Intent(context, Alarm.class).setData(Uri.fromParts(SCHEME, id, null));
    }

    private static PendingIntent pending(Context context, Intent alarm) {
        return PendingIntent.getBroadcast(context, 0, alarm,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    private static void arrive(String json) {
        if (listening) {
            answer(ARRIVAL, json);
        } else {
            waiting.add(json);
        }
    }

    private static Object parsed(String data) {
        if (data == null || data.isEmpty()) {
            return JSONObject.NULL;
        }
        try {
            return new JSONTokener(data).nextValue();
        } catch (JSONException e) {
            return data;
        }
    }

    private static void put(JSONObject out, String key, Object value) {
        try {
            out.put(key, value == null ? "" : value);
        } catch (JSONException e) {
            // A string key and a JSON value never fail.
        }
    }
}
