package org.balaur.google;

import android.app.Activity;
import android.credentials.Credential;
import android.credentials.CredentialManager;
import android.credentials.CredentialOption;
import android.credentials.GetCredentialException;
import android.credentials.GetCredentialRequest;
import android.credentials.GetCredentialResponse;
import android.os.Build;
import android.os.Bundle;
import android.os.CancellationSignal;
import android.os.OutcomeReceiver;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * Sign in with Google through Android's own Credential Manager (Android 14,
 * API 34, and later), with no Google library in the APK.
 *
 * The request carries the bundle keys Google's {@code googleid} library
 * writes for its {@code GetGoogleIdOption} (read off googleid 1.2.1), so
 * Google Play services answers it as it answers that library: with an id
 * token issued for the server's web client id.
 *
 * {@code balaur_google} calls {@link #start} from the game's thread. The answer
 * comes back through {@link #answer}, registered by the same crate, as the
 * JSON map a script's waiter receives.
 */
public final class SignIn {
    private static final String GOOGLE_ID = "com.google.android.libraries.identity.googleid.";
    private static final String ANDROIDX = "androidx.credentials.";
    private static final String TYPE = GOOGLE_ID + "TYPE_GOOGLE_ID_TOKEN_CREDENTIAL";
    // GetCustomCredentialOption's own priority for a Google id option.
    private static final int PRIORITY = 500;
    private static final int CREDENTIAL_MANAGER_API = 34;

    private static final String KIND = "kind";
    private static final String SIGNED_IN = "signed_in";
    private static final String CANCELLED = "cancelled";
    private static final String UNSUPPORTED = "unsupported";
    private static final String ERROR = "error";

    private SignIn() {}

    /** The answer for {@code request}, as JSON. Registered by balaur_google. */
    static native void answer(long request, String json);

    /**
     * Ask Google for an id token for {@code serverClientId}: the accounts
     * already signed in to this app first, without a tap when there is one,
     * then every account on the device.
     */
    public static void start(final Activity activity, final long request, final String serverClientId) {
        if (Build.VERSION.SDK_INT < CREDENTIAL_MANAGER_API) {
            send(request, unsupported("Credential Manager needs Android 14"));
            return;
        }
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                ask(activity, request, serverClientId, true);
            }
        });
    }

    private static void ask(final Activity activity, final long request, final String serverClientId,
            final boolean returning) {
        CredentialManager manager = activity.getSystemService(CredentialManager.class);
        if (manager == null) {
            send(request, unsupported("no Credential Manager on this device"));
            return;
        }
        CredentialOption option = new CredentialOption.Builder(
                TYPE, option(serverClientId, returning), option(serverClientId, returning))
                .setIsSystemProviderRequired(true)
                .build();
        GetCredentialRequest question = new GetCredentialRequest.Builder(new Bundle())
                .addCredentialOption(option)
                .setAlwaysSendAppInfoToProvider(true)
                .build();
        manager.getCredential(activity, question, new CancellationSignal(), activity.getMainExecutor(),
                new OutcomeReceiver<GetCredentialResponse, GetCredentialException>() {
                    @Override
                    public void onResult(GetCredentialResponse response) {
                        send(request, signedIn(response.getCredential()));
                    }

                    @Override
                    public void onError(GetCredentialException error) {
                        String type = error.getType();
                        boolean none = GetCredentialException.TYPE_NO_CREDENTIAL.equals(type);
                        if (none && returning) {
                            ask(activity, request, serverClientId, false);
                        } else if (none) {
                            // No Google account on the device: Google's page
                            // is where the player can sign in to one.
                            send(request, unsupported("no Google account on this device"));
                        } else if (GetCredentialException.TYPE_USER_CANCELED.equals(type)) {
                            send(request, kind(CANCELLED));
                        } else {
                            send(request, failed(type + ": " + error.getMessage()));
                        }
                    }
                });
    }

    /** What GetGoogleIdOption puts in both its request and its query. */
    private static Bundle option(String serverClientId, boolean returning) {
        Bundle data = new Bundle();
        data.putString(GOOGLE_ID + "BUNDLE_KEY_SERVER_CLIENT_ID", serverClientId);
        data.putString(GOOGLE_ID + "BUNDLE_KEY_NONCE", null);
        data.putBoolean(GOOGLE_ID + "BUNDLE_KEY_FILTER_BY_AUTHORIZED_ACCOUNTS", returning);
        data.putString(GOOGLE_ID + "BUNDLE_KEY_LINKED_SERVICE_ID", null);
        data.putStringArrayList(GOOGLE_ID + "BUNDLE_KEY_ID_TOKEN_DEPOSITION_SCOPES", null);
        data.putBoolean(GOOGLE_ID + "BUNDLE_KEY_REQUEST_VERIFIED_PHONE_NUMBER", false);
        data.putBoolean(GOOGLE_ID + "BUNDLE_KEY_AUTO_SELECT_ENABLED", returning);
        Bundle claims = new Bundle();
        claims.putInt(GOOGLE_ID + "BUNDLE_KEY_CLAIMS_SIZE", 0);
        data.putBundle(GOOGLE_ID + "BUNDLE_KEY_CLAIMS", claims);
        data.putString(GOOGLE_ID + "BUNDLE_KEY_HOSTED_DOMAIN_FILTER", null);
        data.putBoolean(ANDROIDX + "BUNDLE_KEY_IS_AUTO_SELECT_ALLOWED", returning);
        data.putInt(ANDROIDX + "BUNDLE_KEY_TYPE_PRIORITY_VALUE", PRIORITY);
        return data;
    }

    private static JSONObject signedIn(Credential credential) {
        if (!TYPE.equals(credential.getType())) {
            return failed("a credential of type " + credential.getType());
        }
        Bundle data = credential.getData();
        JSONObject out = kind(SIGNED_IN);
        try {
            out.put("token", text(data, "BUNDLE_KEY_ID_TOKEN"));
            out.put("email", text(data, "BUNDLE_KEY_ID"));
            out.put("name", text(data, "BUNDLE_KEY_DISPLAY_NAME"));
        } catch (JSONException e) {
            return failed(e.getMessage());
        }
        return out;
    }

    private static String text(Bundle data, String key) {
        String value = data.getString(GOOGLE_ID + key);
        return value == null ? "" : value;
    }

    private static JSONObject kind(String kind) {
        JSONObject out = new JSONObject();
        try {
            out.put(KIND, kind);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    private static JSONObject unsupported(String why) {
        JSONObject out = kind(UNSUPPORTED);
        try {
            out.put(ERROR, why);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    private static JSONObject failed(String message) {
        JSONObject out = kind(ERROR);
        try {
            out.put(ERROR, message == null ? "" : message);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }

    private static void send(long request, JSONObject answer) {
        answer(request, answer.toString());
    }
}
