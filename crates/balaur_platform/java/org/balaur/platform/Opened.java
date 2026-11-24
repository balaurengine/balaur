package org.balaur.platform;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

/**
 * Where a tapped notification lands: it hands {@link Notifications} what the
 * notification carried and brings the game forward, starting it when it was
 * not running. NativeActivity forwards no new intent, so the game hears the
 * tap from here.
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
