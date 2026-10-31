//! An offscreen loop's pace: one frame in flight on the GPU.
//!
//! With no vsync to block on, a loop that never waited ran a thousand frames
//! ahead of the GPU, and the work queued for them held 2.6 MB a frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kiss3d::context::Context;
use kiss3d::wgpu;

/// Whether the last frame's GPU work is done; none before the first.
#[derive(Default)]
pub(crate) struct InFlight(Option<Arc<AtomicBool>>);

impl InFlight {
    /// Wait for the last frame's work, unless it is already done: this
    /// frame's step overlapped it, and only its draw waits.
    pub(crate) fn wait(&mut self) {
        if let Some(done) = self.0.take()
            && !done.load(Ordering::Acquire)
        {
            let _ = Context::get()
                .device
                .poll(wgpu::PollType::wait_indefinitely());
        }
    }

    /// Mark everything submitted so far as this frame's work.
    pub(crate) fn submitted(&mut self) {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);
        Context::get()
            .queue
            .on_submitted_work_done(move || flag.store(true, Ordering::Release));
        self.0 = Some(done);
    }
}
