//! Draws kept past their frame: what a script drew between
//! `begin_retained(key)` and `end_retained()` is held under `key` and drawn
//! every frame until the key is drawn again or cleared.
//!
//! An editor's gizmos and guides change when what they show does, not every
//! frame; drawn this way they are redrawn on the word that says so, and the
//! frame in between only puts the kept lines back in the buffers.

use std::collections::BTreeMap;

use balaur_core::Engine;
use balaur_core::debug_lines::{DebugLine2d, DebugLine3d, DebugLineBuffer2d, DebugLineBuffer3d};
use balaur_script::{Bindings, BindingsExt};

use crate::draw_2d::{DrawBuffer2d, Drawn2d};
use crate::world_text::{TextDraw, TextDrawBuffer};

/// One key's draws, as the buffers hold them.
#[derive(Default, Clone)]
struct Group {
    lines_3d: Vec<DebugLine3d>,
    lines_2d: Vec<DebugLine2d>,
    shapes: Vec<Drawn2d>,
    text: Vec<TextDraw>,
}

/// How long each buffer was when a capture began: what came after is the
/// capture's.
struct Marks {
    key: String,
    lines_3d: usize,
    lines_2d: usize,
    shapes: usize,
    text: usize,
}

/// Every kept group, the capture under way, and the camera the groups were
/// last measured against.
#[derive(Default)]
pub struct RetainedDraws {
    groups: BTreeMap<String, Group>,
    open: Option<Marks>,
    seen: Vec<f32>,
}

/// What the renderer says from no node when the camera a frame was drawn
/// with moved, in either dimension, or the window it was drawn into did: a
/// kept overlay sized in pixels is drawn again on it. Only while someone
/// listens.
pub const CAMERA_MOVED: &str = "camera_moved";

fn len<T: 'static>(eng: &Engine, read: impl Fn(&T) -> usize) -> usize {
    eng.try_resource::<T>()
        .map_or(0, |held| read(&held.borrow()))
}

/// Start keeping what is drawn under `key`. A capture still open is let go:
/// what it drew stays this frame's and nothing more.
pub fn begin(eng: &Engine, key: &str) {
    let marks = Marks {
        key: key.to_string(),
        lines_3d: len(eng, |b: &DebugLineBuffer3d| b.lines.len()),
        lines_2d: len(eng, |b: &DebugLineBuffer2d| b.lines.len()),
        shapes: len(eng, |b: &DrawBuffer2d| b.shapes.len()),
        text: len(eng, |b: &TextDrawBuffer| b.items.len()),
    };
    eng.resource::<RetainedDraws>().borrow_mut().open = Some(marks);
}

/// Keep what was drawn since `begin` under its key, in place of what the key
/// held. Taken out of the buffers: the frame's replay puts it back, so it is
/// drawn once.
pub fn end(eng: &Engine) {
    let held = eng.resource::<RetainedDraws>();
    let Some(marks) = held.borrow_mut().open.take() else {
        return;
    };
    let mut group = Group::default();
    if let Some(b) = eng.try_resource::<DebugLineBuffer3d>() {
        let lines = &mut b.borrow_mut().lines;
        group.lines_3d = lines.split_off(marks.lines_3d.min(lines.len()));
    }
    if let Some(b) = eng.try_resource::<DebugLineBuffer2d>() {
        let lines = &mut b.borrow_mut().lines;
        group.lines_2d = lines.split_off(marks.lines_2d.min(lines.len()));
    }
    if let Some(b) = eng.try_resource::<DrawBuffer2d>() {
        let shapes = &mut b.borrow_mut().shapes;
        group.shapes = shapes.split_off(marks.shapes.min(shapes.len()));
    }
    if let Some(b) = eng.try_resource::<TextDrawBuffer>() {
        let items = &mut b.borrow_mut().items;
        group.text = items.split_off(marks.text.min(items.len()));
    }
    held.borrow_mut().groups.insert(marks.key, group);
}

/// Stop drawing what `key` kept.
pub fn clear(eng: &Engine, key: &str) {
    eng.resource::<RetainedDraws>()
        .borrow_mut()
        .groups
        .remove(key);
}

/// Put every kept group back in the buffers, for the backend to draw and
/// drain with the rest of the frame.
pub(crate) fn replay_system(eng: &Engine, _dt: f32) {
    let Some(held) = eng.try_resource::<RetainedDraws>() else {
        return;
    };
    let held = held.borrow();
    for group in held.groups.values() {
        if let Some(b) = eng.try_resource::<DebugLineBuffer3d>() {
            b.borrow_mut().lines.extend_from_slice(&group.lines_3d);
        }
        if let Some(b) = eng.try_resource::<DebugLineBuffer2d>() {
            b.borrow_mut().lines.extend_from_slice(&group.lines_2d);
        }
        if let Some(b) = eng.try_resource::<DrawBuffer2d>() {
            b.borrow_mut().shapes.extend_from_slice(&group.shapes);
        }
        if let Some(b) = eng.try_resource::<TextDrawBuffer>() {
            b.borrow_mut().items.extend_from_slice(&group.text);
        }
    }
}

/// Say `camera_moved` once the pose a frame was drawn with differs from the
/// last one: the backend publishes it as it draws, so this is the frame
/// after, with the matrices a script projects through already current.
pub(crate) fn camera_moved_system(eng: &Engine, _dt: f32) {
    let mut now = Vec::with_capacity(24);
    if let Some(view) = eng.try_resource::<crate::ViewportSnapshot3d>() {
        let view = view.borrow();
        now.extend_from_slice(&view.view_proj);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window's size in points is exact in f32"
        )]
        now.extend_from_slice(&[view.width as f32, view.height as f32]);
    }
    if let Some(view) = eng.try_resource::<crate::ViewportSnapshot2d>() {
        let view = view.borrow();
        now.extend_from_slice(&[view.center[0], view.center[1], view.zoom]);
    }
    let Some(held) = eng.try_resource::<RetainedDraws>() else {
        return;
    };
    if held.borrow().seen == now {
        return;
    }
    held.borrow_mut().seen = now;
    if balaur_core::events::heard(eng, CAMERA_MOVED) {
        balaur_core::events::emit(eng, CAMERA_MOVED, balaur_script::Value::Nil);
    }
}

pub(crate) fn install_retained_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("begin_retained", &[], "(key: string)", "Keep what is drawn from here to `end_retained` under `key`, drawn every frame until the key is drawn again or cleared: an overlay redrawn when what it shows moves, not each frame."),
        ("end_retained", &[], "()", "Keep what was drawn since `begin_retained` under its key, in place of what the key held."),
        ("clear_retained", &[], "(key: string)", "Stop drawing what `key` kept. A script hears the camera a frame was drawn with move, to draw a group sized in pixels again, with `events::listen(this.node, \"camera_moved\")`."),
    ]);
    m.function("begin_retained", |eng: &Engine, key: String| {
        begin(eng, &key);
        Ok(())
    });
    m.function("end_retained", |eng: &Engine, ()| {
        end(eng);
        Ok(())
    });
    m.function("clear_retained", |eng: &Engine, key: String| {
        clear(eng, &key);
        Ok(())
    });
}
