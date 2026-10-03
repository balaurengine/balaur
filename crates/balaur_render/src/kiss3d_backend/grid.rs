use balaur_core::App;
use kiss3d::prelude::*;

use crate::GridConfig;

/// Ground-plane grid, drawn as per-frame lines on the XZ plane.
pub(super) fn draw_grid(app: &App, window: &mut Window) {
    let Some(grid) = app.engine.try_resource::<GridConfig>() else {
        return;
    };
    let grid = grid.borrow();
    if !grid.enabled {
        return;
    }
    let half = grid.extent as f32 * grid.step;
    let [mr, mg, mb] = grid.minor_color;
    let [jr, jg, jb] = grid.major_color;
    for i in -grid.extent..=grid.extent {
        let offset = i as f32 * grid.step;
        let major = grid.major_every > 0 && i.rem_euclid(grid.major_every.cast_signed()) == 0;
        let color = if major {
            Color::new(jr, jg, jb, 1.0)
        } else {
            Color::new(mr, mg, mb, 1.0)
        };
        let width = if i == 0 { 2.0 } else { 1.0 };
        window.draw_line(
            Vec3::new(offset, 0.0, -half),
            Vec3::new(offset, 0.0, half),
            color,
            width,
            true,
        );
        window.draw_line(
            Vec3::new(-half, 0.0, offset),
            Vec3::new(half, 0.0, offset),
            color,
            width,
            true,
        );
    }
}
