package org.balaur.platform;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.net.Uri;
import android.os.Bundle;

/**
 * A sign-in page in a Custom Tab: {@code platform.web_sign_in} on Android.
 *
 * This activity sits, unseen, between the game and the tab it opens, as
 * AppAuth's management activity does. It hears the tab shut when it resumes
 * with no redirect, and {@link Redirect} brings it back with the URL the page
 * ended on, which clears the tab above it. No AndroidX: the tab is asked for
 * with the session extra the Custom Tabs protocol reads.
 *
 * {@code balaur_platform} calls {@link #start} and {@link #close} from the
 * game's thread; the answer goes back through {@link #answer}, which the same
 * crate registers. Every static here is touched on the UI thread alone.
 */
public final class WebSignIn extends Activity {
    private static final String SESSION = "android.support.customtabs.extra.SESSION";
    private static final String SERVICE = "android.support.customtabs.action.CustomTabsService";
    private static final String URL = "org.balaur.platform.url";
    private static final String CLOSE = "org.balaur.platform.close";
    private static final String LAUNCHED = "launched";
    private static final long NONE = -1;

    private static final String WEB_SIGN_IN = "web_sign_in";
    private static final String CANCELLED = "cancelled";
    private static final String ERROR = "error";

    /** The request the open page answers, or NONE. */
    private static long pending = NONE;
    /** The scheme it ends on, or null. */
    private static String pendingScheme;

    private String url;
    private Uri redirect;
    private boolean launched;
    private boolean closing;

    /** A request's kind, and the URL or the message. Registered by balaur_platform. */
    static native void answer(long request, String kind, String detail);

    /** Show {@code url} in a tab over {@code game}, shutting the page open before. */
    public static void start(final Activity game, final long request, final String url, final String scheme) {
        game.runOnUiThread(() -> {
            settle(CANCELLED, "");
            pending = request;
            pendingScheme = scheme;
            try {
                game.startActivity(back(game).putExtra(URL, url));
            } catch (ActivityNotFoundException e) {
                settle(ERROR, "the template's manifest has no WebSignIn activity");
            }
        });
    }

    /** Bring the game back over the open tab, which answers {@code cancelled}. */
    public static void close(final Activity game) {
        game.runOnUiThread(() -> {
            if (pending != NONE) {
                game.startActivity(back(game).putExtra(CLOSE, true));
            }
        });
    }

    /** Whether {@code url} ends the open page: it is on the scheme the page named. */
    static boolean awaits(Uri url) {
        return pending != NONE && pendingScheme != null && url != null
                && pendingScheme.equalsIgnoreCase(url.getScheme());
    }

    /** The intent that finds this activity in the game's task and clears the tab above it. */
    static Intent back(Activity from) {
        return new Intent(from, WebSignIn.class)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
    }

    private static void settle(String kind, String detail) {
        if (pending == NONE) {
            return;
        }
        long request = pending;
        pending = NONE;
        pendingScheme = null;
        answer(request, kind, detail);
    }

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        take(getIntent());
        // Recreated under the tab (a rotation): the tab is already up.
        launched = saved != null && saved.getBoolean(LAUNCHED);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        take(intent);
    }

    private void take(Intent intent) {
        if (intent.getBooleanExtra(CLOSE, false)) {
            closing = true;
        } else if (intent.getData() != null) {
            redirect = intent.getData();
        } else if (intent.getStringExtra(URL) != null) {
            url = intent.getStringExtra(URL);
            redirect = null;
            launched = false;
            closing = false;
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        // A process the system ended and restarted has no page and no game.
        if (pending == NONE) {
            finish();
        } else if (redirect != null && !closing) {
            settle(WEB_SIGN_IN, redirect.toString());
            finish();
        } else if (closing || launched || url == null) {
            settle(CANCELLED, "");
            finish();
        } else {
            launched = true;
            try {
                startActivity(tab(url));
            } catch (ActivityNotFoundException e) {
                settle(ERROR, "no browser to show the sign-in page");
                finish();
            }
        }
    }

    @Override
    protected void onSaveInstanceState(Bundle out) {
        super.onSaveInstanceState(out);
        out.putBoolean(LAUNCHED, launched);
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        // Cleared by a link into the game (Redirect) without resuming.
        if (isFinishing()) {
            settle(CANCELLED, "");
        }
    }

    /** A Custom Tab for {@code url}: a VIEW intent with the session extra, in a browser that has them. */
    private Intent tab(String url) {
        Intent tab = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
        Bundle session = new Bundle();
        session.putBinder(SESSION, null);
        tab.putExtras(session);
        String browser = browser(getPackageManager());
        if (browser != null) {
            tab.setPackage(browser);
        }
        return tab;
    }

    /** The default browser when it has Custom Tabs, else the first that does, else null. */
    @SuppressWarnings("deprecation") // the ResolveInfoFlags forms need API 33
    private static String browser(PackageManager packages) {
        ResolveInfo preferred = packages.resolveActivity(
                new Intent(Intent.ACTION_VIEW, Uri.parse("https://")), PackageManager.MATCH_DEFAULT_ONLY);
        String first = null;
        for (ResolveInfo service : packages.queryIntentServices(new Intent(SERVICE), 0)) {
            String name = service.serviceInfo.packageName;
            if (preferred != null && preferred.activityInfo != null
                    && name.equals(preferred.activityInfo.packageName)) {
                return name;
            }
            if (first == null) {
                first = name;
            }
        }
        return first;
    }
}
