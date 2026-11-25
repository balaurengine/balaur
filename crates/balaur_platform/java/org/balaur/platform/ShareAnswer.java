package org.balaur.platform;

/**
 * Which answer an open share gets, from the chooser's two reports in either
 * order: the app the player picked ({@link Chosen}) and the chooser shutting
 * ({@link Share}'s activity result).
 *
 * The pick is a broadcast and the result an activity callback, and Android
 * orders neither before the other, so the result often comes first. A pick
 * answers {@code shared} the moment it lands, the result or no result; the
 * result alone waits {@link #WAIT} for a pick still on its way before it
 * answers {@code cancelled}. Plain Java, no Android, so a JVM tests it
 * (balaur_platform's {@code tests/java}). UI thread alone.
 */
final class ShareAnswer {
    /** What a request answers through. */
    interface Answers {
        void shared(long request, String target);

        void cancelled(long request);
    }

    /** Runs {@code body} {@code milliseconds} from now: the main looper's handler. */
    interface Later {
        void run(Runnable body, long milliseconds);
    }

    /** How long a shut chooser waits for its pick: a broadcast lands in tens of milliseconds. */
    static final long WAIT = 2000;
    static final long NONE = -1;

    private final Answers answers;
    private final Later later;
    /** The share waiting for its answer, or NONE. */
    private long open = NONE;

    ShareAnswer(Answers answers, Later later) {
        this.answers = answers;
        this.later = later;
    }

    /** The chooser for {@code request} is up; an earlier share's leftovers no longer count. */
    void opened(long request) {
        open = request;
    }

    /** The player picked {@code target} from the chooser {@code request} opened. */
    void picked(long request, String target) {
        if (request != NONE && request == open) {
            open = NONE;
            answers.shared(request, target);
        }
    }

    /** The chooser {@code request} opened shut, picked from or not. */
    void shut(final long request) {
        if (request == NONE || request != open) {
            return;
        }
        later.run(() -> {
            if (request == open) {
                open = NONE;
                answers.cancelled(request);
            }
        }, WAIT);
    }
}
