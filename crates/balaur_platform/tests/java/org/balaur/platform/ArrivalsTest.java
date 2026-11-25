package org.balaur.platform;

import java.util.ArrayList;
import java.util.List;

/**
 * {@link Arrivals} on a plain JVM: a link or a tap that lands before the
 * game listens, and after. tests/suite/java.rs compiles and runs it; a
 * failed check exits non-zero with what it saw.
 */
public final class ArrivalsTest {
    /** Everything handed on, in order. */
    private final List<String> heard = new ArrayList<>();
    private final Arrivals arrivals = new Arrivals(heard::add);

    private static void check(boolean held, String what, Object saw) {
        if (!held) {
            throw new AssertionError(what + ": saw " + saw);
        }
    }

    private static void whatLandsBeforeTheGameListensWaitsForIt() {
        ArrivalsTest t = new ArrivalsTest();
        t.arrivals.arrive("game://launch");
        t.arrivals.arrive("game://second");
        check(t.heard.isEmpty(), "nothing goes on before listen", t.heard);
        t.arrivals.listen();
        check(t.heard.equals(List.of("game://launch", "game://second")), "listen hands them on in order", t.heard);
    }

    private static void whatLandsAfterGoesStraightOn() {
        ArrivalsTest t = new ArrivalsTest();
        t.arrivals.listen();
        t.arrivals.arrive("game://running");
        check(t.heard.equals(List.of("game://running")), "a link while the game runs", t.heard);
    }

    private static void eachArrivalIsHandedOnOnce() {
        ArrivalsTest t = new ArrivalsTest();
        t.arrivals.arrive("game://launch");
        t.arrivals.listen();
        t.arrivals.listen();
        t.arrivals.arrive("game://launch");
        check(t.heard.equals(List.of("game://launch", "game://launch")),
                "a second listen hands nothing again; the same link opened again is the player's", t.heard);
    }

    public static void main(String[] args) {
        whatLandsBeforeTheGameListensWaitsForIt();
        whatLandsAfterGoesStraightOn();
        eachArrivalIsHandedOnOnce();
        System.out.println("3 checks held");
    }
}
