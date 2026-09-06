//! Drawing the segments a frame asked for, in both dimensions.
//!
//! The buffers are `balaur_core`'s, because more than one producer fills them;
//! this is the half that puts them on screen, kept out of the frame loop so
//! that file stays about the order of a frame.

use balaur_core::App;
use balaur_core::debug_lines::{DebugLineBuffer, DebugLineBuffer2d};
use glamx::{Vec2, Vec3};
use kiss3d::color::Color;
use kiss3d::window::Window;

pub(crate) fn flush_debug_lines_2d(app: &App, window: &mut Window) {
    let Some(lines) = app.engine.try_resource::<DebugLineBuffer2d>() else {
        return;
    };
    for (a, b, c, width) in lines.borrow_mut().lines.drain(..) {
        window.draw_line_2d(
            Vec2::new(a[0], a[1]),
            Vec2::new(b[0], b[1]),
            Color::new(c[0], c[1], c[2], 1.0),
            width,
        );
    }
}

pub(crate) fn flush_debug_lines(app: &App, window: &mut Window) {
    let Some(lines) = app.engine.try_resource::<DebugLineBuffer>() else {
        return;
    };
    for (a, b, c, width, perspective, on_top) in lines.borrow_mut().lines.drain(..) {
        let a = Vec3::new(a[0], a[1], a[2]);
        let b = Vec3::new(b[0], b[1], b[2]);
        let color = Color::new(c[0], c[1], c[2], 1.0);
        if on_top {
            // depth_bias = 1.0 collapses depth to the near plane: the line
            // renders over everything (editor rotation ball, overlays).
            let polyline = kiss3d::renderer::Polyline3d::new(vec![a, b])
                .with_color(color)
                .with_width(width)
                .with_perspective(perspective)
                .with_depth_bias(1.0);
            window.draw_polyline(&polyline);
        } else {
            window.draw_line(a, b, color, width, perspective);
        }
    }
}
