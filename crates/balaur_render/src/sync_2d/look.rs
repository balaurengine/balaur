//! What a 2D slot draws beyond its pose: its sprite's frame and size, a
//! nine-slice mesh, a lean, a polyline's colours and a multimesh's copies.

use balaur_core::hecs::Entity;
use balaur_core::{App, GlobalTransform};
use glamx::Vec2;
use kiss3d::scene::SceneNode2d;

use crate::kiss3d_backend::Slot2d;
use crate::{Renderable2d, Shape2d, SpriteTexture};

/// A `multimesh2d`'s copies onto the node and its overlay companion; answers
/// whether any is drawn, and `true` for a node that carries none.
pub(super) fn draw_multimesh(
    app: &App,
    world: &balaur_core::hecs::World,
    entity: Entity,
    slot: &mut Slot2d,
    renderable: &Renderable2d,
    global: &GlobalTransform,
) -> bool {
    let Ok(multimesh) = world.get::<&crate::MultiMesh>(entity) else {
        return true;
    };
    let drawn = renderable
        .polygon
        .as_ref()
        .and_then(|polygon| crate::texture::size_of(&app.engine, &polygon.texture).ok());
    let any = {
        let mut data = slot.node.data_mut();
        let Some(object) = data.object_mut() else {
            return true;
        };
        crate::instancing::draw_multimesh_2d(object, &multimesh, global, drawn)
    };
    // The shear cached with the old instance is gone with it.
    slot.shear = f32::NAN;
    // The companion sits at the node's origin, so the same copies in its own
    // space land where the node's do.
    if let Some(companion) = slot.companion.as_mut()
        && let Some(object) = companion.data_mut().object_mut()
    {
        crate::instancing::draw_multimesh_2d(object, &multimesh, global, drawn);
    }
    any
}

/// A sprite's size and frame onto its node: the nine-slice mesh it bakes,
/// or the unit quad's UVs. Answers the scale the node takes for its size.
pub(super) fn frame_sprite(app: &App, slot: &mut Slot2d, renderable: &Renderable2d) -> Vec2 {
    // A sprite is the one 2D shape still built at unit size: its extents
    // come from the image and change without rebuilding the node. A
    // nine-slice one bakes its size, since its corners must not stretch.
    let nine = nine_of(app, renderable, &slot.node, false);
    if slot.nine != nine {
        set_nine_mesh(&mut slot.node, nine.as_ref());
        slot.nine = nine;
    }
    let size = match renderable.shape {
        Shape2d::Sprite { .. } if nine.is_some() => Vec2::ONE,
        Shape2d::Sprite { hx, hy } => Vec2::new(2.0 * hx, 2.0 * hy),
        _ => Vec2::ONE,
    };
    // Every sync, not just on rebuild: frames and flips are UV changes, so
    // an animation that flips frames must not rebuild the node.
    if let Some(sprite) = renderable.sprite.as_ref().filter(|_| nine.is_none()) {
        let flip = (sprite.flip_x, sprite.flip_y);
        if sprite.sheet.is_some() || sprite.region.is_some() || flip != slot.flip {
            // The rectangle is in the pixels the image was drawn at, which
            // a shrunk copy no longer has: `size_of` answers those.
            let drawn = sprite
                .region
                .and_then(|_| crate::texture::size_of(&app.engine, &sprite.path).ok());
            sync_sprite_uvs(&mut slot.node, sprite, drawn);
        }
        slot.flip = flip;
    }
    size
}

/// Whether a renderable puts anything on screen. A sprite with no image and
/// no material draws nothing, as Godot's does: a script clears the texture to
/// hide what it showed. It keeps its placeholder size for picking.
pub(super) fn draws(renderable: &Renderable2d, inherited: balaur_core::scene::MaterialId) -> bool {
    renderable.sprite.as_ref().is_none_or(|sprite| {
        !sprite.path.is_empty() || !renderable.material.is_empty() || !inherited.is_none()
    })
}

/// Lean a node by its shear and answer how far its sprite's quad sits off
/// it: the shift turns and scales with the node, so it is taken through both.
pub(super) fn lean_and_shift(
    slot: &mut Slot2d,
    renderable: &Renderable2d,
    global: &GlobalTransform,
) -> Vec2 {
    // The shear is the node's one instance, applied between rotation and
    // scale as Godot does; a polyline's group node has no object to lean.
    if slot.shear.to_bits() != global.skew.to_bits() && slot.node.data().object().is_some() {
        let (sin, cos) = (
            balaur_core::libm::sinf(global.skew),
            balaur_core::libm::cosf(global.skew),
        );
        slot.node.set_instances(&[kiss3d::scene::InstanceData2d {
            deformation: glamx::Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(-sin, cos)),
            ..Default::default()
        }]);
        slot.shear = global.skew;
    }
    match (&renderable.sprite, renderable.shape) {
        (Some(sprite), Shape2d::Sprite { hx, hy }) => {
            let [x, y] = sprite.centre(hx, hy);
            (global.affine_2d() * glamx::Vec3::new(x, y, 0.0)).truncate()
        }
        _ => Vec2::ZERO,
    }
}

/// A node's own colour with the tint every ancestor contributed multiplied
/// in, channel by channel, alpha included.
pub(crate) fn modulate(color: [f32; 4], tint: [f32; 4]) -> [f32; 4] {
    [
        color[0] * tint[0],
        color[1] * tint[1],
        color[2] * tint[2],
        color[3] * tint[3],
    ]
}

/// A polyline's pieces carry their own colour: the tint blended toward the
/// gradient by where each sits, since a group's colour stops at the group.
pub(crate) fn tint_pieces(slot: &mut Slot2d, renderable: &Renderable2d, inherited: [f32; 4]) {
    let [r, g, b, a] = modulate(renderable.color, inherited);
    let end = renderable
        .line
        .as_ref()
        .and_then(|style| style.gradient)
        .map_or([r, g, b, a], |gradient| modulate(gradient, inherited));
    for (piece, along) in &mut slot.pieces {
        let t = *along;
        // Mixed as written and decoded after, so the midpoint is the one a
        // picker shows between the two ends.
        piece.set_color(crate::film::color([
            r + (end[0] - r) * t,
            g + (end[1] - g) * t,
            b + (end[2] - b) * t,
            a + (end[3] - a) * t,
        ]));
    }
}

/// Remap the node's UVs to the sprite's sheet frame, with the U or V extents
/// swapped for flips.
pub(crate) fn sync_sprite_uvs(
    node: &mut SceneNode2d,
    sprite: &SpriteTexture,
    drawn: Option<(u32, u32)>,
) {
    let texture = node.data().object().map(|o| o.data().texture().size);
    let (min, max) = sprite_uv_rect(sprite, drawn, texture);
    node.set_uv_rect(min, max);
}

/// The corner of the image a sprite draws, in the mesh's own UVs.
///
/// `drawn` is the size a region is measured against and `texture` the size of
/// the image as it was uploaded, which is what the sheet's sliver is in.
pub(crate) fn sprite_uv_rect(
    sprite: &SpriteTexture,
    drawn: Option<(u32, u32)>,
    texture: Option<(u32, u32)>,
) -> (Vec2, Vec2) {
    let sheet = sprite
        .sheet
        .map(|s| kiss3d::scene::SpriteSheet::new(s.columns, s.rows));
    let (mut min, mut max) = sheet.map_or((Vec2::ZERO, Vec2::ONE), |s| s.frame_uv(sprite.frame));
    // A region is a rectangle of the image in pixels; it wins over a sheet.
    if let Some([x, y, w, h]) = sprite.region {
        let size = drawn.or(texture);
        if let Some((tw, th)) = size.filter(|(tw, th)| *tw > 0 && *th > 0) {
            let (tw, th) = (tw as f32, th as f32);
            min = Vec2::new(x as f32 / tw, y as f32 / th);
            max = Vec2::new((x + w) as f32 / tw, (y + h) as f32 / th);
        }
    }
    if sheet.is_some() && sprite.region.is_none() {
        // The sliver keeps a nearest-sampled edge fragment from rounding into
        // the neighbouring frame, matching kiss3d's own `set_sprite_frame`.
        if let Some((w, h)) = texture
            && w > 1
            && h > 1
        {
            let inset = Vec2::new(0.05 / w as f32, 0.05 / h as f32);
            min += inset;
            max -= inset;
        }
    }
    if sprite.flip_x {
        std::mem::swap(&mut min.x, &mut max.x);
    }
    if sprite.flip_y {
        std::mem::swap(&mut min.y, &mut max.y);
    }
    (min, max)
}

/// The nine-slice mesh a sprite draws, or `None` for one drawn as a plain
/// quad. `whole` builds it over the whole image, for a run whose members
/// each pick their own rectangle in their instance.
pub(crate) fn nine_of(
    app: &App,
    renderable: &Renderable2d,
    node: &SceneNode2d,
    whole: bool,
) -> Option<crate::sprite::NineKey> {
    let sprite = renderable.sprite.as_ref()?;
    let margins = sprite.nine?;
    let Shape2d::Sprite { hx, hy } = renderable.shape else {
        return None;
    };
    let drawn = crate::texture::size_of(&app.engine, &sprite.path).ok();
    let cell = match (sprite.region, sprite.sheet, drawn) {
        (Some([_, _, w, h]), _, _) | (None, None, Some((w, h))) => (w as f32, h as f32),
        (None, Some(sheet), Some((w, h))) => (
            w as f32 / sheet.columns.max(1) as f32,
            h as f32 / sheet.rows.max(1) as f32,
        ),
        _ => (1.0, 1.0),
    };
    let rect = if whole {
        let mut rect = kiss3d::scene::UV_WHOLE_2D;
        if sprite.flip_x {
            rect.swap(0, 2);
        }
        if sprite.flip_y {
            rect.swap(1, 3);
        }
        rect
    } else {
        let uploaded = node.data().object().map(|o| o.data().texture().size);
        let (min, max) = sprite_uv_rect(sprite, sprite.region.and(drawn), uploaded);
        [min.x, min.y, max.x, max.y]
    };
    Some(crate::sprite::nine_key(
        margins,
        (hx, hy),
        renderable.pixels_per_unit,
        cell,
        rect,
    ))
}

/// The mesh a nine-slice key describes: kiss3d's own nine-slice, its UVs
/// carried into the rectangle the sprite draws.
pub(crate) fn nine_mesh(key: &crate::sprite::NineKey) -> kiss3d::resource::GpuMesh2d {
    use kiss3d::scene::Border;
    let border = |[left, right, top, bottom]: [f32; 4]| Border {
        left,
        right,
        top,
        bottom,
    };
    let mesh = kiss3d::scene::nine_slice_mesh(
        Vec2::from_array(key.size),
        border(key.world),
        border(key.uv),
    );
    let [x0, y0, x1, y1] = key.rect;
    if let Some(uvs) = mesh
        .uvs()
        .write()
        .ok()
        .as_mut()
        .and_then(|u| u.data_mut().as_mut())
    {
        for uv in uvs.iter_mut() {
            *uv = Vec2::new(x0 + uv.x * (x1 - x0), y0 + uv.y * (y1 - y0));
        }
    }
    mesh
}

/// Give a sprite's node the nine-slice mesh it now draws, or a plain unit
/// quad back. The quad kiss3d hands sprites is shared, so it is copied first.
pub(super) fn set_nine_mesh(node: &mut SceneNode2d, key: Option<&crate::sprite::NineKey>) {
    let mesh = match key {
        Some(key) => nine_mesh(key),
        None => kiss3d::scene::nine_slice_mesh(
            Vec2::ONE,
            kiss3d::scene::Border::uniform(0.0),
            kiss3d::scene::Border::uniform(0.0),
        ),
    };
    let mut mesh = Some(mesh);
    node.apply_to_object_mut(&mut |object| {
        object.make_mesh_unique();
        if let Some(mesh) = mesh.take() {
            *object.mesh().borrow_mut() = mesh;
        }
    });
}
