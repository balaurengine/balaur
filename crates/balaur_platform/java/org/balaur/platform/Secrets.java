package org.balaur.platform;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.security.GeneralSecurityException;
import java.security.KeyStore;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/**
 * {@code secrets.*} on Android: each value sealed with AES-GCM under a key
 * the Android Keystore holds and never hands out, kept in the app's private
 * preferences as the nonce and the sealed bytes, in base64. A value sealed
 * under a key the device no longer has (a backup restored elsewhere) reads
 * as nothing.
 */
public final class Secrets {
    private static final String KEYSTORE = "AndroidKeyStore";
    private static final String ALIAS = "org.balaur.platform.secrets";
    private static final String PREFERENCES = "org.balaur.platform.secrets";
    private static final String TRANSFORMATION = "AES/GCM/NoPadding";
    private static final int NONCE_BYTES = 12;
    private static final int TAG_BITS = 128;

    private Secrets() {}

    public static boolean set(Context context, String key, String value) {
        try {
            Cipher cipher = Cipher.getInstance(TRANSFORMATION);
            cipher.init(Cipher.ENCRYPT_MODE, sealingKey());
            byte[] nonce = cipher.getIV();
            byte[] sealed = cipher.doFinal(value.getBytes(StandardCharsets.UTF_8));
            byte[] kept = new byte[nonce.length + sealed.length];
            System.arraycopy(nonce, 0, kept, 0, nonce.length);
            System.arraycopy(sealed, 0, kept, nonce.length, sealed.length);
            return preferences(context)
                    .edit()
                    .putString(key, Base64.encodeToString(kept, Base64.NO_WRAP))
                    .commit();
        } catch (GeneralSecurityException | IOException e) {
            return false;
        }
    }

    public static String get(Context context, String key) {
        String kept = preferences(context).getString(key, null);
        if (kept == null) {
            return null;
        }
        try {
            byte[] bytes = Base64.decode(kept, Base64.NO_WRAP);
            if (bytes.length <= NONCE_BYTES) {
                return null;
            }
            Cipher cipher = Cipher.getInstance(TRANSFORMATION);
            cipher.init(Cipher.DECRYPT_MODE, sealingKey(), new GCMParameterSpec(TAG_BITS, bytes, 0, NONCE_BYTES));
            byte[] opened = cipher.doFinal(bytes, NONCE_BYTES, bytes.length - NONCE_BYTES);
            return new String(opened, StandardCharsets.UTF_8);
        } catch (GeneralSecurityException | IOException | IllegalArgumentException e) {
            return null;
        }
    }

    public static boolean remove(Context context, String key) {
        SharedPreferences preferences = preferences(context);
        if (!preferences.contains(key)) {
            return false;
        }
        return preferences.edit().remove(key).commit();
    }

    private static SharedPreferences preferences(Context context) {
        return context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
    }

    /** The Keystore's key, made the first time a value is sealed. */
    private static SecretKey sealingKey() throws GeneralSecurityException, IOException {
        KeyStore store = KeyStore.getInstance(KEYSTORE);
        store.load(null);
        KeyStore.Entry entry = store.getEntry(ALIAS, null);
        if (entry instanceof KeyStore.SecretKeyEntry) {
            return ((KeyStore.SecretKeyEntry) entry).getSecretKey();
        }
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE);
        generator.init(new KeyGenParameterSpec.Builder(
                        ALIAS, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build());
        return generator.generateKey();
    }
}
