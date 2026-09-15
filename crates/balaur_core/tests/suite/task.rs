//! Stepped work: that a slice at a time finishes, that the count is honest,
//! and that the pump advances what a browser parks.
//!
//! The native `task::step` hands the job a thread, so what is tested directly
//! is the pump and the counter, and the thread path through its own answer.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use balaur_core::task::{self, Progress, Stepped};
use balaur_core::{App, AppConfig};

/// A job that counts down, recording each slice it ran.
struct Countdown {
    left: usize,
    slices: Rc<std::cell::RefCell<Vec<usize>>>,
}

impl Stepped for Countdown {
    fn step(&mut self) -> Progress {
        self.slices.borrow_mut().push(self.left);
        self.left -= 1;
        if self.left == 0 {
            Progress::Done
        } else {
            Progress::More
        }
    }
}

/// A job that is `Send`, for the thread the desktop puts it on.
struct Ticker {
    left: usize,
    ran: &'static AtomicUsize,
}

impl Stepped for Ticker {
    fn step(&mut self) -> Progress {
        self.ran.fetch_add(1, Ordering::Relaxed);
        self.left -= 1;
        if self.left == 0 {
            Progress::Done
        } else {
            Progress::More
        }
    }
}

static RAN: AtomicUsize = AtomicUsize::new(0);

/// One `pump` is one slice, whatever the job has left: this is what keeps a
/// tab painting through an import rather than freezing for all of it.
#[test]
fn the_pump_advances_a_parked_job_one_slice_at_a_time() {
    let app = App::new(AppConfig::bare(".")).unwrap();
    let slices = Rc::new(std::cell::RefCell::new(Vec::new()));
    // `park` is what a browser's `step` is, and a native caller may ask for
    // it too: the job stays on this thread and the pump advances it.
    task::park(Countdown {
        left: 3,
        slices: slices.clone(),
    });

    task::advance_parked_system(&app.engine, 0.0);
    assert_eq!(*slices.borrow(), vec![3], "one slice per pump");
    task::advance_parked_system(&app.engine, 0.0);
    assert_eq!(*slices.borrow(), vec![3, 2]);
    task::advance_parked_system(&app.engine, 0.0);
    assert_eq!(*slices.borrow(), vec![3, 2, 1]);
    // Done, so it is dropped rather than stepped again.
    task::advance_parked_system(&app.engine, 0.0);
    assert_eq!(*slices.borrow(), vec![3, 2, 1]);
}

/// A job on a thread runs to the end without anything pumping it.
#[test]
fn a_job_handed_a_thread_runs_to_completion() {
    RAN.store(0, Ordering::Relaxed);
    task::step(Ticker { left: 5, ran: &RAN });
    // Waited on the job's own count rather than on `running()`, which is
    // every job in the process and so is not this test's to assert.
    #[allow(
        clippy::disallowed_methods,
        reason = "a timeout on a thread this test waits for, outside any simulation"
    )]
    let start = std::time::Instant::now();
    while RAN.load(Ordering::Relaxed) < 5 && start.elapsed().as_secs() < 5 {
        std::thread::yield_now();
    }
    assert_eq!(RAN.load(Ordering::Relaxed), 5, "every slice ran, and once");
}
