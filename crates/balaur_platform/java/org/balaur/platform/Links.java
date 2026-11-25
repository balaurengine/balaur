package org.balaur.platform;

import android.app.Activity;

import org.json.JSONException;
import org.json.JSONObject;

/**
 * Links into the game: {@code url_opened} on {@code platform_event} on
 * Android. {@link Redirect} hands over each link on one of the game's
 * schemes ({@code [android] schemes}) that ends no sign-in page, as it
 * lands; the link that started the process waits until the game
 * {@link #listen}s, so it arrives too.
 *
 * {@code balaur_platform} calls {@link #listen} from the game's thread and
 * registers {@link #answer}. Every static here is touched on the UI thread alone.
 */
public final class Links {
    private static final String OPENED = "url_opened";
    private static final String KEY_URL = "url";
    /** What arrives unasked carries the request no call is given. */
    private static final long ARRIVAL = 0;

    private static final Arrivals arrivals = new Arrivals(json -> answer(ARRIVAL, json));

    private Links() {}

    /** Under request 0, an arrival as JSON. Registered by balaur_platform. */
    static native void answer(long request, String json);

    /** Hand over the link that started the game, and every one that lands from now on. */
    public static void listen(final Activity game) {
        game.runOnUiThread(arrivals::listen);
    }

    /** A link into the game landed on {@link Redirect}: its whole URL. */
    static void arrive(String url) {
        JSONObject arrival = Device.kind(OPENED);
        try {
            arrival.put(KEY_URL, url);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        arrivals.arrive(arrival.toString());
    }
}
