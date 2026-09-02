//! Line segments a frame wants drawn, and nothing about how they are drawn.
//!
//! The buffers live here rather than in `balaur_render` because more than one
//! plugin fills them and only one drains them: scripts through
//! `render.draw_line`, physics through its own debug draw, the editor through
//! its gizmos. A producer that had to depend on the renderer to draw a line
//! would invert the plugin layering for a `Vec<f32>`.
//!
//! Appended during a frame, drained by whatever is drawing — and cleared even
//! when nothing is, so a headless run does not grow one of these forever.

/// (a, b, color, pixel width, perspective-correct width, always-on-top)
pub type DebugLine = ([f32; 3], [f32; 3], [f32; 3], f32, bool, bool);

/// Scripts append with `render.draw_line`; the windowed backend drains it as
/// it draws, and with no window the render plugin's Render-stage system
/// clears it instead, so it is empty at the start of every frame either way.
#[derive(Default)]
pub struct DebugLineBuffer {
    pub lines: Vec<DebugLine>,
}

/// 2D counterpart of [`DebugLineBuffer`]: world-space 2D segments rendered
/// with the 2D camera.
/// (a, b, color, pixel width)
pub type DebugLine2d = ([f32; 2], [f32; 2], [f32; 3], f32);

/// Appended by `render.draw_line_2d`, drained on the same terms as
/// [`DebugLineBuffer`].
#[derive(Default)]
pub struct DebugLineBuffer2d {
    pub lines: Vec<DebugLine2d>,
}

impl DebugLineBuffer {
    /// One segment, at the default width, depth-tested.
    pub fn push(&mut self, a: [f32; 3], b: [f32; 3], color: [f32; 3]) {
        self.lines.push((a, b, color, 1.0, false, false));
    }
}

impl DebugLineBuffer2d {
    pub fn push(&mut self, a: [f32; 2], b: [f32; 2], color: [f32; 3]) {
        self.lines.push((a, b, color, 1.0));
    }
}
