package org.balaur.platform;

import android.content.BroadcastReceiver;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;

/**
 * The app the player picked from {@link Share}'s chooser, which the system
 * reports here with the request the chooser was opened for.
 */
public final class Chosen extends BroadcastReceiver {
    @Override
    @SuppressWarnings("deprecation") // the typed getParcelableExtra needs API 33
    public void onReceive(Context context, Intent intent) {
        ComponentName picked = intent.getParcelableExtra(Intent.EXTRA_CHOSEN_COMPONENT);
        if (picked != null) {
            Share.ANSWER.picked(intent.getLongExtra(Share.REQUEST, ShareAnswer.NONE), picked.getPackageName());
        }
    }
}
