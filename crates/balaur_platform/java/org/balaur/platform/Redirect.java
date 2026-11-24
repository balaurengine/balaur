package org.balaur.platform;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

/**
 * Where a URL on one of the game's schemes lands ({@code [android] schemes},
 * the export's intent filters): back to {@link WebSignIn} when it ends the
 * open page, which clears the tab, else to the game, as a link into it.
 */
public final class Redirect extends Activity {
    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        Uri url = getIntent().getData();
        Intent next = WebSignIn.awaits(url)
                ? WebSignIn.back(this)
                : getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (next != null) {
            next.setData(url);
            next.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            startActivity(next);
        }
        finish();
    }
}
