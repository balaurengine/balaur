package org.balaur.platform;

import android.Manifest;
import android.app.Activity;
import android.content.pm.PackageManager;
import android.os.Bundle;

/**
 * The POST_NOTIFICATIONS prompt (Android 13 and later), from an unseen
 * activity: NativeActivity forwards no permission result, so this asks and
 * answers {@link Notifications}.
 */
public final class Permission extends Activity {
    private static final int ASKED = 1;

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        // Recreated under the prompt (a rotation): it is already up.
        if (saved == null) {
            requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, ASKED);
        }
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        boolean granted = results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED;
        Notifications.allowed(getIntent().getLongExtra(Notifications.REQUEST, -1), granted);
        finish();
    }
}
