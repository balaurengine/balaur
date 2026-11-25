package org.balaur.platform;

import java.util.ArrayList;
import java.util.List;

/**
 * What a shim hears unasked, held until the game listens: a tap or a link
 * that started the process lands before the native library could take it,
 * so it waits here, and everything after goes straight on. Each arrival is
 * handed on once. Plain Java, no Android, so a JVM tests it
 * (balaur_platform's {@code tests/java}). UI thread alone.
 */
final class Arrivals {
    /** Where an arrival goes once the game listens: the shim's native method. */
    interface Sink {
        void arrive(String arrival);
    }

    private final Sink sink;
    /** What arrived before the game listened. */
    private final List<String> waiting = new ArrayList<>();
    /** Whether the game takes arrivals yet. */
    private boolean listening;

    Arrivals(Sink sink) {
        this.sink = sink;
    }

    /** Hand {@code arrival} on, or keep it until the game listens. */
    void arrive(String arrival) {
        if (listening) {
            sink.arrive(arrival);
        } else {
            waiting.add(arrival);
        }
    }

    /** Hand over what arrived, and everything that arrives from now on. */
    void listen() {
        listening = true;
        List<String> held = new ArrayList<>(waiting);
        waiting.clear();
        for (String arrival : held) {
            sink.arrive(arrival);
        }
    }
}
