package org.balaur.platform;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/**
 * Where a tapped notification lands: it hands {@link Notifications} what the
 * notification carried and brings the game forward, starting it when it was
 * not running. A running {@link Game} takes the launch as a new intent, and
 * what was over it (a sign-in page, the share sheet) is cleared, so a tap
 * never starts a second game. The tap is heard here, before the game resumes.
 */
public final class Opened extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        Intent tap = getIntent();
        Notifications.opened(tap.getStringExtra(Notifications.ID), tap.getStringExtra(Notifications.DATA));
        Intent game = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (game != null) {
            game.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(game);
        }
        finish();
    }
}
