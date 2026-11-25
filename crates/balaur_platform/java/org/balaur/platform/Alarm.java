package org.balaur.platform;

import android.app.ActivityManager;
import android.app.Notification;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Bundle;

/**
 * A scheduled notification coming due: posted on the game's channel, with a
 * tap that opens {@link Opened}. One posted while the game is in front is
 * also an arrival, as Apple's is.
 */
public final class Alarm extends BroadcastReceiver {
    /** The application meta-data the export names the small icon in. */
    private static final String SMALL_ICON = "org.balaur.platform.notification_icon";

    @Override
    public void onReceive(Context context, Intent alarm) {
        post(context, alarm);
    }

    static void post(Context context, Intent alarm) {
        Notifications.channel(context);
        String id = alarm.getStringExtra(Notifications.ID);
        String title = alarm.getStringExtra(Notifications.TITLE);
        String body = alarm.getStringExtra(Notifications.BODY);
        String data = alarm.getStringExtra(Notifications.DATA);
        Intent tap = new Intent(context, Opened.class)
                .setData(alarm.getData())
                .putExtra(Notifications.ID, id)
                .putExtra(Notifications.DATA, data)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        PendingIntent open = PendingIntent.getActivity(context, 0, tap,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification shown = new Notification.Builder(context, Notifications.CHANNEL)
                .setSmallIcon(smallIcon(context))
                .setContentTitle(title)
                .setContentText(body)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build();
        Notifications.manager(context).notify(id, Notifications.ONE, shown);
        if (inFront()) {
            Notifications.received(id, title, body, data);
        }
    }

    /**
     * The white silhouette the export wrote for {@code [android] notification_icon},
     * else the system's reminder: an adaptive launcher icon as a small icon
     * crashes Android 8's status bar.
     */
    @SuppressWarnings("deprecation") // the ApplicationInfoFlags form needs API 33
    static int smallIcon(Context context) {
        int icon = 0;
        try {
            Bundle meta = context.getPackageManager()
                    .getApplicationInfo(context.getPackageName(), PackageManager.GET_META_DATA).metaData;
            icon = meta == null ? 0 : meta.getInt(SMALL_ICON);
        } catch (PackageManager.NameNotFoundException e) {
            // The game's own package is always there.
        }
        return icon != 0 ? icon : android.R.drawable.ic_popup_reminder;
    }

    private static boolean inFront() {
        ActivityManager.RunningAppProcessInfo process = new ActivityManager.RunningAppProcessInfo();
        ActivityManager.getMyMemoryState(process);
        return process.importance == ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND;
    }
}
