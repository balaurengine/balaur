package org.balaur.google;

import android.app.Activity;

import com.google.android.play.core.review.ReviewManager;
import com.google.android.play.core.review.ReviewManagerFactory;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * Play's in-app review prompt behind {@code device.request_review}. Play
 * decides whether it shows, and never says: the call answers {@code done}
 * once the flow is over, shown or not.
 */
public final class Review {
    private static final String KIND = "kind";
    private static final String ERROR = "error";

    private Review() {}

    /** A request's answer, as JSON. Registered by balaur_platform. */
    static native void answer(long request, String json);

    /** Whether this template carries Play's review library: the class loading is the answer. */
    public static boolean available(Activity game) {
        return game != null;
    }

    /** Ask Play for its review flow, and run it over the game. */
    public static void start(final Activity game, final long request, final String call, final String args) {
        game.runOnUiThread(() -> {
            ReviewManager manager = ReviewManagerFactory.create(game);
            manager.requestReviewFlow().addOnCompleteListener(asked -> {
                if (!asked.isSuccessful()) {
                    answer(request, failed(String.valueOf(asked.getException())).toString());
                    return;
                }
                manager.launchReviewFlow(game, asked.getResult()).addOnCompleteListener(shown -> {
                    JSONObject out = new JSONObject();
                    try {
                        out.put(KIND, "done");
                        out.put("call", call);
                    } catch (JSONException e) {
                        // A string key and value never fail.
                    }
                    answer(request, out.toString());
                });
            });
        });
    }

    private static JSONObject failed(String message) {
        JSONObject out = new JSONObject();
        try {
            out.put(KIND, ERROR);
            out.put(ERROR, message);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }
}
