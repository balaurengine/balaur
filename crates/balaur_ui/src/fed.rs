//! Pointer and finger input a script fed, handed to egui as a pass begins, so
//! a widget answers `input.set_touch` as it does a finger the window reported.
//!
//! One path for every host: an egui plugin's input hook runs inside
//! `Context::begin_pass`, which the headless pass, a test's own pass and the
//! window's all call. Only script-fed events travel it, so a windowed run
//! never hands egui twice what the window already reported.

use balaur_core::Engine;
use balaur_input::{FedPointer, InputSnapshot, TouchPhase};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What scripts fed since the last pass began, in window pixels, in the
/// order fed.
#[derive(Default)]
pub(crate) struct FedQueue(Arc<Mutex<Vec<FedPointer>>>);

/// The plugin that drains [`FedQueue`] into each pass's input.
struct FedInput {
    queue: Arc<Mutex<Vec<FedPointer>>>,
    /// The fed finger driving egui's pointer: the first one down keeps it
    /// until it lifts, as the window's first finger does.
    finger: Option<u64>,
    /// Where the fed pointer last was, in points, for a button to press at.
    pointer: Option<egui::Pos2>,
}

/// Take this frame's fed pointer and finger events into the queue.
pub(crate) fn queue_system(eng: &Engine, _: f32) {
    // A replay's widgets hear the recorded `WidgetInputSnapshot`, and a re-run
    // tick queued its fingers on its first run.
    if balaur_core::replay::suppressed(eng) {
        return;
    }
    let (Some(input), Some(queue)) = (
        eng.try_resource::<InputSnapshot>(),
        eng.try_resource::<FedQueue>(),
    ) else {
        return;
    };
    let input = input.borrow();
    let queue = queue.borrow();
    // With no context holding the queue, nothing would ever drain it.
    if input.fed_pointer().is_empty() || Arc::strong_count(&queue.0) == 1 {
        return;
    }
    lock(&queue.0).extend_from_slice(input.fed_pointer());
}

/// Whether fed input waits for a pass, which a lazy UI runs for as it does
/// for the window's.
pub(crate) fn waiting(eng: &Engine) -> bool {
    eng.try_resource::<FedQueue>()
        .is_some_and(|queue| !lock(&queue.borrow().0).is_empty())
}

/// Give `ctx` the plugin that hands it fed input, once.
pub(crate) fn hand_to(eng: &Engine, ctx: &egui::Context) {
    let Some(queue) = eng.try_resource::<FedQueue>() else {
        return;
    };
    let queue = Arc::clone(&queue.borrow().0);
    // A context another engine drew with before takes this engine's queue.
    let held = ctx.with_plugin(|plugin: &mut FedInput| {
        if !Arc::ptr_eq(&plugin.queue, &queue) {
            plugin.queue = Arc::clone(&queue);
        }
    });
    if held.is_none() {
        ctx.add_plugin(FedInput {
            queue,
            finger: None,
            pointer: None,
        });
    }
}

fn lock(queue: &Mutex<Vec<FedPointer>>) -> MutexGuard<'_, Vec<FedPointer>> {
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

impl egui::Plugin for FedInput {
    fn debug_name(&self) -> &'static str {
        "balaur fed input"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let fed = std::mem::take(&mut *lock(&self.queue));
        if fed.is_empty() {
            return;
        }
        // What `begin_pass` is about to lay the pass out at: the zoom times
        // the display's own scale, as the window divides its pixels by.
        let native = input
            .viewports
            .get(&input.viewport_id)
            .and_then(|viewport| viewport.native_pixels_per_point)
            .unwrap_or(1.0);
        let per_point = ctx.zoom_factor() * native;
        let modifiers = ctx.input(|i| i.modifiers);
        for one in fed {
            match one {
                FedPointer::Moved(x, y) => {
                    let pos = egui::pos2(x / per_point, y / per_point);
                    self.pointer = Some(pos);
                    input.events.push(egui::Event::PointerMoved(pos));
                }
                FedPointer::Button(index, pressed) => {
                    let pos = self
                        .pointer
                        .or_else(|| ctx.input(|i| i.pointer.latest_pos()));
                    if let (Some(button), Some(pos)) = (button_of(index), pos) {
                        input.events.push(egui::Event::PointerButton {
                            pos,
                            button,
                            pressed,
                            modifiers,
                        });
                    }
                }
                FedPointer::Touch(id, x, y, phase) => {
                    let pos = egui::pos2(x / per_point, y / per_point);
                    self.touch(id, pos, phase, &mut input.events);
                }
                FedPointer::Scroll(dx, dy) => input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(dx, dy),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers,
                }),
            }
        }
    }
}

impl FedInput {
    /// A finger as a touch screen reports it: the raw touch, for pinches, and
    /// for the finger driving the pointer, the press, drag and lift a mouse
    /// would make. A cancel lets the pointer go without a release, so it
    /// never clicks.
    fn touch(
        &mut self,
        id: u64,
        pos: egui::Pos2,
        phase: TouchPhase,
        events: &mut Vec<egui::Event>,
    ) {
        events.push(egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(id),
            phase: match phase {
                TouchPhase::Start => egui::TouchPhase::Start,
                TouchPhase::Move => egui::TouchPhase::Move,
                TouchPhase::End => egui::TouchPhase::End,
                TouchPhase::Cancel => egui::TouchPhase::Cancel,
            },
            pos,
            force: None,
        });
        let drives = self.finger == Some(id);
        match phase {
            TouchPhase::Start if self.finger.is_none() => {
                self.finger = Some(id);
                self.pointer = Some(pos);
                events.push(egui::Event::PointerMoved(pos));
                events.push(primary(pos, true));
            }
            TouchPhase::Move if drives => {
                self.pointer = Some(pos);
                events.push(egui::Event::PointerMoved(pos));
            }
            TouchPhase::End if drives => {
                self.finger = None;
                events.push(primary(pos, false));
                events.push(egui::Event::PointerGone);
            }
            TouchPhase::Cancel if drives => {
                self.finger = None;
                events.push(egui::Event::PointerGone);
            }
            _ => {}
        }
    }
}

fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

/// The egui button for a `MOUSE_BUTTON_*` index: the three a window hands
/// egui, so a fed back or forward button reaches the widgets no more than a
/// real one does.
const fn button_of(index: usize) -> Option<egui::PointerButton> {
    match index {
        0 => Some(egui::PointerButton::Primary),
        1 => Some(egui::PointerButton::Secondary),
        2 => Some(egui::PointerButton::Middle),
        _ => None,
    }
}
