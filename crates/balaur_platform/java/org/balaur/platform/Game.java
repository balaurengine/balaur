package org.balaur.platform;

import android.app.NativeActivity;
import android.content.Intent;

/**
 * The game's own activity: NativeActivity, declared {@code singleTop}, so a
 * notification's tap or a link into the game reaches the running game as a
 * new intent rather than a second activity under a live native library.
 *
 * NativeActivity drops a new intent; this makes it the activity's own, and an
 * activity always resumes after one, so a shim reads it as it resumes, the
 * way it reads the intent of a cold start.
 */
public class Game extends NativeActivity {
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }
}
