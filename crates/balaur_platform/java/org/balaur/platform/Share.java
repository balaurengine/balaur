package org.balaur.platform;

import android.app.Activity;
import android.app.PendingIntent;
import android.content.ClipData;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;

import java.io.File;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * The share sheet: {@code device.share} on Android.
 *
 * An unseen activity over the game, as {@link WebSignIn} is, because only an
 * activity hears the chooser shut. The chooser reports the app the player
 * picked to {@link Chosen}, which may land before or after it shuts;
 * {@link ShareAnswer} answers {@code shared} with it, or {@code cancelled}
 * when none came. A file is handed over as a content URI from {@link Files},
 * read-only.
 */
public final class Share extends Activity {
    static final String REQUEST = "org.balaur.platform.request";
    private static final String TEXT = "org.balaur.platform.text";
    private static final String FILE = "org.balaur.platform.file";
    private static final String TITLE = "org.balaur.platform.title";
    private static final String LAUNCHED = "launched";
    private static final String PLAIN = "text/plain";
    private static final int CHOOSER = 1;
    private static final int MUTABLE_FLAG_API = 31;

    private static final String SHARED = "shared";
    private static final String CANCELLED = "cancelled";
    private static final String TARGET = "target";

    /** The open share's answer, from whichever of the chooser's reports settles it. */
    static final ShareAnswer ANSWER = new ShareAnswer(new ShareAnswer.Answers() {
        @Override
        public void shared(long request, String target) {
            JSONObject answer = Device.kind(SHARED);
            try {
                answer.put(TARGET, target);
            } catch (JSONException e) {
                // A string key and value never fail.
            }
            Device.answer(request, answer.toString());
        }

        @Override
        public void cancelled(long request) {
            Device.answer(request, Device.kind(CANCELLED).toString());
        }
    }, new Handler(Looper.getMainLooper())::postDelayed);

    private boolean launched;
    private boolean shut;

    static Intent intent(Context game, long request, String text, String file, String title) {
        return new Intent(game, Share.class)
                .putExtra(REQUEST, request)
                .putExtra(TEXT, text)
                .putExtra(FILE, file)
                .putExtra(TITLE, title);
    }

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        // Recreated under the chooser (a rotation): it is already up.
        launched = saved != null && saved.getBoolean(LAUNCHED);
        if (launched) {
            return;
        }
        launched = true;
        try {
            startActivityForResult(chooser(), CHOOSER);
            ANSWER.opened(request());
        } catch (RuntimeException e) {
            Device.answer(request(), Device.failed(e.getMessage()).toString());
            finish();
        }
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        shut();
        finish();
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        // Cleared by a tap bringing the game forward, with no result.
        if (isFinishing()) {
            shut();
        }
    }

    private void shut() {
        if (!shut) {
            shut = true;
            ANSWER.shut(request());
        }
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        out.putBoolean(LAUNCHED, launched);
    }

    private long request() {
        return getIntent().getLongExtra(REQUEST, -1);
    }

    private Intent chooser() {
        Intent asked = getIntent();
        String text = asked.getStringExtra(TEXT);
        String file = asked.getStringExtra(FILE);
        String title = asked.getStringExtra(TITLE);
        Intent send = new Intent(Intent.ACTION_SEND);
        if (file != null && !file.isEmpty()) {
            Uri uri = Files.uri(this, new File(file));
            send.setType(Files.type(file));
            send.putExtra(Intent.EXTRA_STREAM, uri);
            send.setClipData(ClipData.newRawUri("", uri));
            send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        } else {
            send.setType(PLAIN);
        }
        if (text != null && !text.isEmpty()) {
            send.putExtra(Intent.EXTRA_TEXT, text);
        }
        boolean titled = title != null && !title.isEmpty();
        if (titled) {
            send.putExtra(Intent.EXTRA_TITLE, title);
        }
        // The chooser fills the picked component in, so the intent is mutable.
        int flags = PendingIntent.FLAG_UPDATE_CURRENT
                | (Build.VERSION.SDK_INT >= MUTABLE_FLAG_API ? PendingIntent.FLAG_MUTABLE : 0);
        Intent chosen = new Intent(this, Chosen.class).putExtra(REQUEST, request());
        PendingIntent picked = PendingIntent.getBroadcast(this, 0, chosen, flags);
        return Intent.createChooser(send, titled ? title : null, picked.getIntentSender());
    }
}
