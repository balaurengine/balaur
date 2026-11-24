package org.balaur.google;

import android.app.Activity;

import com.google.android.gms.common.ConnectionResult;
import com.google.android.gms.common.GoogleApiAvailability;
import com.google.android.gms.common.api.ApiException;
import com.google.android.gms.games.AnnotatedData;
import com.google.android.gms.games.GamesClientStatusCodes;
import com.google.android.gms.games.LeaderboardsClient;
import com.google.android.gms.games.PageDirection;
import com.google.android.gms.games.PlayGames;
import com.google.android.gms.games.PlayGamesSdk;
import com.google.android.gms.games.Player;
import com.google.android.gms.games.SnapshotsClient;
import com.google.android.gms.games.achievement.Achievement;
import com.google.android.gms.games.achievement.AchievementBuffer;
import com.google.android.gms.games.leaderboard.LeaderboardScore;
import com.google.android.gms.games.leaderboard.LeaderboardScoreBuffer;
import com.google.android.gms.games.leaderboard.LeaderboardVariant;
import com.google.android.gms.games.snapshot.Snapshot;
import com.google.android.gms.games.snapshot.SnapshotMetadataChange;
import com.google.android.gms.tasks.Task;

import java.io.IOException;
import java.nio.charset.StandardCharsets;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Play Games Services v2 behind {@code platform.*}: sign-in, the player,
 * achievements, leaderboards, the one-time server code and saved games.
 *
 * {@code balaur_google} calls {@link #start} from the game's thread with a
 * call's name and its arguments as JSON. Each answers through
 * {@link #answer}, registered by the same crate, as the JSON map a script's
 * waiter receives; request 0 is what nobody asked for, the sign-in Play
 * makes by itself as the game starts.
 */
public final class Games {
    // The words balaur_platform's `kind` module and calls spell.
    private static final String KIND = "kind";
    private static final String ERROR = "error";
    private static final String CALL = "call";
    private static final String DONE = "done";
    private static final String SIGNED_IN = "signed_in";
    private static final String UNSUPPORTED = "unsupported";

    // A leaderboard page holds at most this many rows.
    private static final int PAGE = 25;

    private static boolean begun;

    private Games() {}

    /** A request's answer, as JSON. Registered by balaur_google. */
    static native void answer(long request, String json);

    /** Whether Play services is on this device to answer. */
    public static boolean available(Activity game) {
        return GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(game)
                == ConnectionResult.SUCCESS;
    }

    /** Start {@code call} for {@code request}, with its arguments as JSON. */
    public static void start(final Activity game, final long request, final String call, final String args) {
        game.runOnUiThread(() -> {
            try {
                dispatch(game, request, call, new JSONObject(args));
            } catch (JSONException | RuntimeException e) {
                answer(request, failed(call + ": " + e).toString());
            }
        });
    }

    private static void dispatch(Activity game, long request, String call, JSONObject args) throws JSONException {
        switch (call) {
            case "begin":
                begin(game);
                break;
            case "sign_in":
                signIn(game, request);
                break;
            case "unlock":
                report(request, call, PlayGames.getAchievementsClient(game).unlockImmediate(args.getString("achievement")));
                break;
            case "progress":
                progress(game, request, args.getString("achievement"), args.getDouble("percent"));
                break;
            case "submit_score":
                report(request, call, PlayGames.getLeaderboardsClient(game)
                        .submitScoreImmediate(args.getString("board"), args.getLong("score")));
                break;
            case "scores":
                scores(game, request, args);
                break;
            case "identity":
                identity(game, request, args.getString("server_client_id"));
                break;
            case "cloud_read":
                read(game, request, args.getString("key"));
                break;
            case "cloud_write":
                write(game, request, args.getString("key"), args.getString("value"));
                break;
            default:
                JSONObject out = kind(UNSUPPORTED);
                out.put(CALL, call);
                answer(request, out.toString());
        }
    }

    /**
     * Play signs the player in by itself as the game starts; once it has,
     * who it is arrives under request 0.
     */
    private static void begin(Activity game) {
        if (begun) {
            return;
        }
        begun = true;
        PlayGamesSdk.initialize(game);
        PlayGames.getGamesSignInClient(game).isAuthenticated().addOnCompleteListener(task -> {
            if (task.isSuccessful() && task.getResult().isAuthenticated()) {
                player(game, 0);
            }
        });
    }

    private static void signIn(Activity game, long request) {
        PlayGamesSdk.initialize(game);
        PlayGames.getGamesSignInClient(game).isAuthenticated().addOnCompleteListener(known -> {
            if (known.isSuccessful() && known.getResult().isAuthenticated()) {
                player(game, request);
                return;
            }
            PlayGames.getGamesSignInClient(game).signIn().addOnCompleteListener(asked -> {
                if (asked.isSuccessful() && asked.getResult().isAuthenticated()) {
                    player(game, request);
                } else {
                    answer(request, failed("the player did not sign in to Play Games").toString());
                }
            });
        });
    }

    private static void player(Activity game, long request) {
        PlayGames.getPlayersClient(game).getCurrentPlayer().addOnCompleteListener(task -> {
            if (!task.isSuccessful()) {
                answer(request, failed(why(task)).toString());
                return;
            }
            Player player = task.getResult();
            try {
                JSONObject who = new JSONObject();
                who.put("id", player.getPlayerId());
                who.put("alias", player.getDisplayName());
                JSONObject out = kind(SIGNED_IN);
                out.put("player", who);
                out.put("id", player.getPlayerId());
                out.put("alias", player.getDisplayName());
                answer(request, out.toString());
            } catch (JSONException e) {
                answer(request, failed(e.toString()).toString());
            }
        });
    }

    /**
     * An incremental achievement takes its share of its steps; a standard
     * one unlocks at 100 and keeps nothing below it.
     */
    private static void progress(Activity game, long request, String id, double percent) {
        if (percent >= 100) {
            report(request, "progress", PlayGames.getAchievementsClient(game).unlockImmediate(id));
            return;
        }
        PlayGames.getAchievementsClient(game).load(false).addOnCompleteListener(task -> {
            if (!task.isSuccessful()) {
                answer(request, failed(why(task)).toString());
                return;
            }
            AnnotatedData<AchievementBuffer> data = task.getResult();
            AchievementBuffer buffer = data.get();
            int total = -1;
            if (buffer != null) {
                for (Achievement achievement : buffer) {
                    if (achievement.getAchievementId().equals(id)
                            && achievement.getType() == Achievement.TYPE_INCREMENTAL) {
                        total = achievement.getTotalSteps();
                    }
                }
                buffer.release();
            }
            if (total < 0) {
                answer(request, done("progress").toString());
                return;
            }
            int steps = (int) Math.round(Math.max(0, percent) / 100.0 * total);
            if (steps == 0) {
                answer(request, done("progress").toString());
                return;
            }
            report(request, "progress", PlayGames.getAchievementsClient(game).setStepsImmediate(id, steps));
        });
    }

    private static void scores(Activity game, long request, JSONObject args) throws JSONException {
        final String board = args.getString("board");
        final int start = Math.max(1, args.getInt("start"));
        final int count = Math.max(0, args.getInt("count"));
        int span = LeaderboardVariant.TIME_SPAN_ALL_TIME;
        if ("today".equals(args.getString("period"))) {
            span = LeaderboardVariant.TIME_SPAN_DAILY;
        } else if ("week".equals(args.getString("period"))) {
            span = LeaderboardVariant.TIME_SPAN_WEEKLY;
        }
        int collection = "friends".equals(args.getString("scope"))
                ? LeaderboardVariant.COLLECTION_FRIENDS
                : LeaderboardVariant.COLLECTION_PUBLIC;
        final int wanted = start - 1 + count;
        final LeaderboardsClient client = PlayGames.getLeaderboardsClient(game);
        final JSONArray entries = new JSONArray();
        client.loadTopScores(board, span, collection, Math.max(1, Math.min(PAGE, wanted)))
                .addOnCompleteListener(task -> page(client, request, board, start, wanted, entries, task));
    }

    /** One page of a leaderboard into {@code entries}, then the next until enough. */
    private static void page(LeaderboardsClient client, long request, String board, int start, int wanted,
            JSONArray entries, Task<AnnotatedData<LeaderboardsClient.LeaderboardScores>> task) {
        if (!task.isSuccessful()) {
            answer(request, failed(why(task)).toString());
            return;
        }
        LeaderboardsClient.LeaderboardScores scores = task.getResult().get();
        LeaderboardScoreBuffer buffer = scores == null ? null : scores.getScores();
        int before = entries.length();
        try {
            if (buffer != null) {
                for (int i = before; i < buffer.getCount() && i < wanted; i++) {
                    LeaderboardScore score = buffer.get(i);
                    JSONObject row = new JSONObject();
                    row.put("player", score.getScoreHolder() == null ? "" : score.getScoreHolder().getPlayerId());
                    row.put("alias", score.getScoreHolderDisplayName());
                    row.put("rank", score.getRank());
                    row.put("score", score.getRawScore());
                    entries.put(row);
                }
            }
            if (buffer != null && entries.length() < wanted && entries.length() > before) {
                // The buffer grows by the page: the next answer holds every row so far.
                client.loadMoreScores(buffer, Math.min(PAGE, wanted - entries.length()), PageDirection.NEXT)
                        .addOnCompleteListener(next -> page(client, request, board, start, wanted, entries, next));
                return;
            }
            JSONArray kept = new JSONArray();
            for (int i = start - 1; i < entries.length(); i++) {
                kept.put(entries.get(i));
            }
            JSONObject out = kind("scores");
            out.put("board", board);
            out.put("entries", kept);
            answer(request, out.toString());
            if (scores != null) {
                scores.release();
            }
        } catch (JSONException e) {
            answer(request, failed(e.toString()).toString());
        }
    }

    private static void identity(Activity game, long request, String serverClientId) {
        if (serverClientId.isEmpty()) {
            JSONObject out = kind(UNSUPPORTED);
            try {
                out.put(CALL, "identity");
                out.put(ERROR, "no [google] server_client_id in project.toml");
            } catch (JSONException e) {
                // A string key and value never fail.
            }
            answer(request, out.toString());
            return;
        }
        PlayGames.getGamesSignInClient(game).requestServerSideAccess(serverClientId, false)
                .addOnCompleteListener(task -> {
                    if (!task.isSuccessful()) {
                        answer(request, failed(why(task)).toString());
                        return;
                    }
                    JSONObject out = kind("identity");
                    try {
                        out.put("code", task.getResult());
                    } catch (JSONException e) {
                        // A string key and value never fail.
                    }
                    answer(request, out.toString());
                });
    }

    /** A saved game's contents, or nil where there is none; a conflict resolves to the newest. */
    private static void read(Activity game, long request, String key) {
        final SnapshotsClient client = PlayGames.getSnapshotsClient(game);
        client.open(key, false, SnapshotsClient.RESOLUTION_POLICY_MOST_RECENTLY_MODIFIED)
                .addOnCompleteListener(task -> {
                    JSONObject out = kind("read");
                    try {
                        out.put("key", key);
                        if (!task.isSuccessful()) {
                            if (status(task) != GamesClientStatusCodes.SNAPSHOT_NOT_FOUND) {
                                answer(request, failed(why(task)).toString());
                                return;
                            }
                            out.put("value", JSONObject.NULL);
                            answer(request, out.toString());
                            return;
                        }
                        Snapshot snapshot = task.getResult().getData();
                        byte[] bytes = snapshot.getSnapshotContents().readFully();
                        client.discardAndClose(snapshot);
                        out.put("value", new String(bytes, StandardCharsets.UTF_8));
                        answer(request, out.toString());
                    } catch (JSONException | IOException e) {
                        answer(request, failed(e.toString()).toString());
                    }
                });
    }

    /**
     * Write a saved game, unless another device wrote one Play has not
     * reconciled with this one's: that conflict is refused, not overwritten.
     */
    private static void write(Activity game, long request, String key, String value) {
        final SnapshotsClient client = PlayGames.getSnapshotsClient(game);
        client.open(key, true, SnapshotsClient.RESOLUTION_POLICY_MANUAL).addOnCompleteListener(task -> {
            if (!task.isSuccessful()) {
                answer(request, failed(why(task)).toString());
                return;
            }
            SnapshotsClient.DataOrConflict<Snapshot> opened = task.getResult();
            if (opened.isConflict()) {
                answer(request, failed("conflict: another device saved " + key
                        + " since this one read it; read it again before writing").toString());
                return;
            }
            Snapshot snapshot = opened.getData();
            snapshot.getSnapshotContents().writeBytes(value.getBytes(StandardCharsets.UTF_8));
            report(request, "cloud_write", client.commitAndClose(snapshot, SnapshotMetadataChange.EMPTY_CHANGE));
        });
    }

    /** `done` for {@code call} once {@code task} succeeds, else its error. */
    private static void report(long request, String call, Task<?> task) {
        task.addOnCompleteListener(finished -> answer(request,
                (finished.isSuccessful() ? done(call) : failed(why(finished))).toString()));
    }

    private static int status(Task<?> task) {
        Exception error = task.getException();
        return error instanceof ApiException ? ((ApiException) error).getStatusCode() : -1;
    }

    private static String why(Task<?> task) {
        Exception error = task.getException();
        return error == null ? "Play Games did not answer" : error.toString();
    }

    private static JSONObject done(String call) {
        JSONObject out = kind(DONE);
        try {
            out.put(CALL, call);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
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

    private static JSONObject failed(String message) {
        JSONObject out = kind(ERROR);
        try {
            out.put(ERROR, message == null ? "" : message);
        } catch (JSONException e) {
            // A string key and value never fail.
        }
        return out;
    }
}
