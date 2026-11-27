package org.balaur.platform;

import android.app.Activity;
import android.content.ContentResolver;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.MediaStore;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * The photo picker: {@code device.pick_photo} on Android.
 *
 * An unseen activity over the game, as {@link Share} is, because only an
 * activity hears what was picked. Android 13 and later show the system's
 * photo picker; earlier ones a document chooser for one image. The picture
 * is copied into the app's cache and answered as {@code picked} with that
 * file's path, which the engine reads and deletes, or {@code cancelled} when
 * the picker shut with none. A HEIF picture is written as a JPEG, which the
 * engine's image decoder reads.
 */
public final class PickPhoto extends Activity {
    private static final String REQUEST = "org.balaur.platform.request";
    private static final String LAUNCHED = "launched";
    private static final String IMAGES = "image/*";
    private static final String JPEG = "image/jpeg";
    private static final String HEIC = "image/heic";
    private static final String HEIF = "image/heif";
    private static final String ANY = "application/octet-stream";
    private static final String PICKED = "picked";
    private static final String CANCELLED = "cancelled";
    private static final String PATH = "path";
    private static final String TYPE = "type";
    private static final int PICK = 1;
    private static final int PICK_IMAGES_API = 33;
    private static final int JPEG_QUALITY = 95;
    private static final int BUFFER_BYTES = 64 * 1024;

    private boolean launched;
    private boolean answered;

    static Intent intent(Context game, long request) {
        return new Intent(game, PickPhoto.class).putExtra(REQUEST, request);
    }

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        // Recreated under the picker (a rotation): it is already up.
        launched = saved != null && saved.getBoolean(LAUNCHED);
        if (launched) {
            return;
        }
        launched = true;
        try {
            startActivityForResult(picker(), PICK);
        } catch (RuntimeException e) {
            answer(Device.failed(e.getMessage()).toString());
            finish();
        }
    }

    private static Intent picker() {
        if (Build.VERSION.SDK_INT >= PICK_IMAGES_API) {
            return new Intent(MediaStore.ACTION_PICK_IMAGES).setType(IMAGES);
        }
        return new Intent(Intent.ACTION_GET_CONTENT).setType(IMAGES).addCategory(Intent.CATEGORY_OPENABLE);
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        Uri picked = result == RESULT_OK && data != null ? data.getData() : null;
        answer(picked == null ? Device.kind(CANCELLED).toString() : copied(picked));
        finish();
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        // Cleared by a tap bringing the game forward, with no result.
        if (isFinishing()) {
            answer(Device.kind(CANCELLED).toString());
        }
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        out.putBoolean(LAUNCHED, launched);
    }

    private void answer(String json) {
        if (!answered) {
            answered = true;
            Device.answer(getIntent().getLongExtra(REQUEST, -1), json);
        }
    }

    /** The picture in the cache, as the answer naming it, or why it is not. */
    private String copied(Uri picked) {
        ContentResolver resolver = getContentResolver();
        String type = resolver.getType(picked);
        File file = new File(getCacheDir(), "balaur-picked-" + getIntent().getLongExtra(REQUEST, -1));
        try {
            if (HEIC.equals(type) || HEIF.equals(type)) {
                writeJpeg(resolver, picked, file);
                type = JPEG;
            } else {
                copy(resolver, picked, file);
            }
        } catch (IOException | SecurityException e) {
            return Device.failed(e.getMessage()).toString();
        }
        JSONObject answer = Device.kind(PICKED);
        try {
            answer.put(PATH, file.getAbsolutePath());
            answer.put(TYPE, type == null ? ANY : type);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return answer.toString();
    }

    private static void copy(ContentResolver resolver, Uri picked, File file) throws IOException {
        try (InputStream in = resolver.openInputStream(picked);
                OutputStream out = new FileOutputStream(file)) {
            if (in == null) {
                throw new IOException("the picture could not be opened");
            }
            byte[] buffer = new byte[BUFFER_BYTES];
            int read;
            while ((read = in.read(buffer)) > 0) {
                out.write(buffer, 0, read);
            }
        }
    }

    private static void writeJpeg(ContentResolver resolver, Uri picked, File file) throws IOException {
        Bitmap bitmap;
        try (InputStream in = resolver.openInputStream(picked)) {
            bitmap = BitmapFactory.decodeStream(in);
        }
        if (bitmap == null) {
            throw new IOException("the picture could not be decoded");
        }
        try (OutputStream out = new FileOutputStream(file)) {
            bitmap.compress(Bitmap.CompressFormat.JPEG, JPEG_QUALITY, out);
        }
    }
}
