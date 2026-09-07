//! The quad a tile cell becomes: its four corners, and the four texture
//! coordinates that carry its picture, turned by the cell's flags.

use balaur_core::tiles::TileSet;

/// The four corners of a quad around a centre, clockwise from the top left.
pub(crate) fn corners_of(centre: glamx::Vec2, half: glamx::Vec2) -> [glamx::Vec2; 4] {
    [
        centre + glamx::Vec2::new(-half.x, half.y),
        centre + glamx::Vec2::new(half.x, half.y),
        centre + glamx::Vec2::new(half.x, -half.y),
        centre + glamx::Vec2::new(-half.x, -half.y),
    ]
}

/// A sheet rect as the four texture coordinates a quad wants.
pub(crate) fn rect_uvs(rect: [f32; 4], sheet: glamx::Vec2, inset: glamx::Vec2) -> [glamx::Vec2; 4] {
    let [x, y, w, h] = rect;
    let min = glamx::Vec2::new(x / sheet.x, y / sheet.y) + inset;
    let max = glamx::Vec2::new((x + w) / sheet.x, (y + h) / sheet.y) - inset;
    [
        glamx::Vec2::new(min.x, min.y),
        glamx::Vec2::new(max.x, min.y),
        glamx::Vec2::new(max.x, max.y),
        glamx::Vec2::new(min.x, max.y),
    ]
}

/// The four corners of a tile on the sheet, in the order the quad above
/// wants them, turned by the cell's flags.
pub(crate) fn tile_uvs(
    set: &TileSet,
    id: u32,
    sheet: glamx::Vec2,
    inset: glamx::Vec2,
    flags: u8,
) -> [glamx::Vec2; 4] {
    let mut corners = rect_uvs(set.tile_rect(id), sheet, inset);
    if flags & balaur_core::tiles::TRANSPOSE != 0 {
        corners.swap(1, 3);
    }
    if flags & balaur_core::tiles::FLIP_X != 0 {
        corners.swap(0, 1);
        corners.swap(2, 3);
    }
    if flags & balaur_core::tiles::FLIP_Y != 0 {
        corners.swap(0, 3);
        corners.swap(1, 2);
    }
    corners
}
