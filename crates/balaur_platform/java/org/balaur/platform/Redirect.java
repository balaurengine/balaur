package org.balaur.platform;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

/**
 * Where a URL on one of the game's schemes lands ({@code [android] schemes},
 * the export's intent filters): back to {@link WebSignIn} when it ends the
 * open page, which clears the tab, else to {@link Links} as a link into the
 * game, which it brings forward, starting it when it was not running. A link
 * is one or the other, never both, and is heard here once: a task reopened
 * from Recents replays the intent it began with, which was heard then.
 */
public final class Redirect extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        Intent link = getIntent();
        boolean replayed = (link.getFlags() & Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY) != 0;
        Uri url = replayed ? null : link.getData();
        Intent next;
        if (WebSignIn.awaits(url)) {
            next = WebSignIn.back(this).setData(url);
        } else {
            if (url != null) {
                Links.arrive(url.toString());
            }
            next = getPackageManager().getLaunchIntentForPackage(getPackageName());
        }
        if (next != null) {
            next.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(next);
        }
        finish();
    }
}
