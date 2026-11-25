package org.balaur.platform;

import java.util.ArrayList;
import java.util.List;

/**
 * {@link ShareAnswer} on a plain JVM: the chooser's pick and its shutting in
 * both orders. tests/suite/java.rs compiles and runs it; a failed check exits
 * non-zero with what it saw.
 */
public final class ShareAnswerTest {
    /** Every answer given, in order, as "shared 7 org.example" or "cancelled 7". */
    private final List<String> answered = new ArrayList<>();
    /** What waits to run later, and how long each asked to wait. */
    private final List<Runnable> waiting = new ArrayList<>();
    private final List<Long> waits = new ArrayList<>();

    private final ShareAnswer answer = new ShareAnswer(new ShareAnswer.Answers() {
        @Override
        public void shared(long request, String target) {
            answered.add("shared " + request + " " + target);
        }

        @Override
        public void cancelled(long request) {
            answered.add("cancelled " + request);
        }
    }, (body, milliseconds) -> {
        waiting.add(body);
        waits.add(milliseconds);
    });

    private void elapse() {
        List<Runnable> due = new ArrayList<>(waiting);
        waiting.clear();
        for (Runnable body : due) {
            body.run();
        }
    }

    private static void check(boolean held, String what, Object saw) {
        if (!held) {
            throw new AssertionError(what + ": saw " + saw);
        }
    }

    private static void aPickBeforeTheChooserShutIsShared() {
        ShareAnswerTest t = new ShareAnswerTest();
        t.answer.opened(7);
        t.answer.picked(7, "org.example.mail");
        t.answer.shut(7);
        t.elapse();
        check(t.answered.equals(List.of("shared 7 org.example.mail")), "a pick, then the result", t.answered);
    }

    private static void aPickAfterTheChooserShutIsStillShared() {
        ShareAnswerTest t = new ShareAnswerTest();
        t.answer.opened(7);
        t.answer.shut(7);
        check(t.answered.isEmpty(), "the result waits for a pick", t.answered);
        check(t.waits.equals(List.of(ShareAnswer.WAIT)), "it waits WAIT", t.waits);
        t.answer.picked(7, "org.example.chat");
        t.elapse();
        check(t.answered.equals(List.of("shared 7 org.example.chat")), "the result, then a pick", t.answered);
    }

    private static void aChooserShutWithNoPickIsCancelledOnceTheWaitIsOver() {
        ShareAnswerTest t = new ShareAnswerTest();
        t.answer.opened(7);
        t.answer.shut(7);
        t.answer.shut(7);
        t.elapse();
        t.answer.picked(7, "org.example.late");
        check(t.answered.equals(List.of("cancelled 7")), "no pick within the wait", t.answered);
    }

    private static void anEarlierSharesPickDoesNotAnswerTheNextOne() {
        ShareAnswerTest t = new ShareAnswerTest();
        t.answer.opened(7);
        t.answer.shut(7);
        t.answer.opened(8);
        t.answer.picked(7, "org.example.stale");
        t.answer.picked(ShareAnswer.NONE, "org.example.unasked");
        t.elapse();
        check(t.answered.isEmpty(), "7's pick and wait leave 8 open", t.answered);
        t.answer.picked(8, "org.example.mail");
        check(t.answered.equals(List.of("shared 8 org.example.mail")), "8 answers its own pick", t.answered);
    }

    public static void main(String[] args) {
        aPickBeforeTheChooserShutIsShared();
        aPickAfterTheChooserShutIsStillShared();
        aChooserShutWithNoPickIsCancelledOnceTheWaitIsOver();
        anEarlierSharesPickDoesNotAnswerTheNextOne();
        System.out.println("4 checks held");
    }
}
