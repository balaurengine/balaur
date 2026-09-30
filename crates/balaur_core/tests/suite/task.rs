//! Stepped work: that a slice at a time finishes, that the count is honest,
//! and that the pump advances what a browser parks. Then the pool the native
//! `task::step` and `task::compute` run on: its limit, its order, a drop and
//! a job that panics.

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

/// How long a test waits on a pool before calling it stuck.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(5);

#[test]
fn a_pool_runs_no_more_than_its_limit_at_once_and_runs_everything() {
    use std::sync::Arc;
    let pool = task::Pool::new("test-limit");
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (done, finished) = std::sync::mpsc::channel();
    for _ in 0..20 {
        let (active, peak, done) = (active.clone(), peak.clone(), done.clone());
        pool.submit(
            move || {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                let _ = done.send(());
            },
            3,
        );
    }
    for job in 0..20 {
        assert!(finished.recv_timeout(PATIENCE).is_ok(), "job {job} ran");
    }
    assert_eq!(peak.load(Ordering::SeqCst), 3, "three at once, never more");
}

#[test]
fn a_pool_of_one_runs_jobs_in_the_order_they_were_queued() {
    let pool = task::Pool::new("test-order");
    let (done, finished) = std::sync::mpsc::channel();
    for job in 0..10 {
        let done = done.clone();
        pool.submit(
            move || {
                let _ = done.send(job);
            },
            1,
        );
    }
    let order: Vec<i32> = (0..10)
        .map(|_| finished.recv_timeout(PATIENCE).unwrap())
        .collect();
    assert_eq!(order, (0..10).collect::<Vec<_>>());
}

#[test]
fn a_dropped_pool_drops_the_jobs_still_waiting() {
    use std::sync::Arc;
    let pool = task::Pool::new("test-drop");
    let (release, held) = std::sync::mpsc::channel::<()>();
    let (started, running) = std::sync::mpsc::channel();
    pool.submit(
        move || {
            let _ = started.send(());
            let _ = held.recv();
        },
        1,
    );
    running.recv_timeout(PATIENCE).unwrap();
    let ran = Arc::new(AtomicUsize::new(0));
    for _ in 0..5 {
        let ran = ran.clone();
        pool.submit(
            move || {
                ran.fetch_add(1, Ordering::SeqCst);
            },
            1,
        );
    }
    assert_eq!(pool.waiting(), 5, "five queued behind the one running");
    drop(pool);
    release.send(()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(ran.load(Ordering::SeqCst), 0, "none of the queued ones ran");
}

#[test]
fn a_job_that_panics_leaves_the_pool_working() {
    let pool = task::Pool::new("test-panic");
    pool.submit(|| panic!("a job gone wrong, on purpose"), 1);
    let (done, finished) = std::sync::mpsc::channel();
    pool.submit(
        move || {
            let _ = done.send(());
        },
        1,
    );
    assert!(
        finished.recv_timeout(PATIENCE).is_ok(),
        "the one worker lived to run the next job"
    );
}

#[test]
fn compute_answers_on_its_channel() {
    let answer = task::compute(|| 6 * 7);
    assert_eq!(answer.recv_timeout(PATIENCE), Ok(42));
}

/// A job that notes the thread it ran on, for the step pool's bound.
struct Named {
    threads: std::sync::mpsc::Sender<String>,
}

impl Stepped for Named {
    fn step(&mut self) -> Progress {
        let name = std::thread::current()
            .name()
            .unwrap_or_default()
            .to_string();
        let _ = self.threads.send(name);
        Progress::Done
    }
}

#[test]
fn stepped_work_runs_on_a_bounded_set_of_named_threads() {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let (threads, heard) = std::sync::mpsc::channel();
    for _ in 0..cores * 4 {
        task::step(Named {
            threads: threads.clone(),
        });
    }
    let names: std::collections::BTreeSet<String> = (0..cores * 4)
        .map(|_| heard.recv_timeout(PATIENCE).unwrap())
        .collect();
    assert!(
        names.iter().all(|name| name.starts_with("balaur-step-")),
        "{names:?}"
    );
    assert!(
        names.len() <= cores,
        "{} threads for {cores} cores",
        names.len()
    );
}
