//! The flags a 3D drawn node carries for itself rather than for its
//! material: the segmentation id and whether shadows land on it. How a node
//! blends, culls and is depth-tested is its material's `[surface]`, in
//! either dimension.
//!
//! The keys are the same on every renderable of a dimension, so their schema
//! lines, reader and read-back are written once here and each component
//! appends them. They are pushed onto the scene with the node's
//! [`crate::debug_draw::DebugDraw`], which is a component of its own.

use balaur_core::components::as_f64;

#[cfg(feature = "window")]
use crate::debug_draw::DebugDraw;
#[cfg(feature = "window")]
use crate::material::Surface;
use crate::vocabulary::keys as k;

/// The flags every 3D renderable takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay3d {
    /// Zero keeps the id kiss3d hands every object.
    pub segmentation_id: u32,
    pub receive_shadows: bool,
}

impl Default for Overlay3d {
    fn default() -> Self {
        Self {
            segmentation_id: 0,
            receive_shadows: true,
        }
    }
}

/// The flag rows a 3D renderable appends to its schema.
pub(crate) fn schema_3d() -> Vec<(&'static str, String)> {
    vec![
        (k::RECEIVE_SHADOWS, r#"{ type = "bool", default = true, description = "Whether shadows land on it; off lights it as if nothing stood between it and every light", group = "shadows" }"#.to_string()),
        (k::SEGMENTATION_ID, r#"{ type = "int", default = 0, min = 0, description = "The id `render.snap_aov(\"segmentation\")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up", group = "layers" }"#.to_string()),
    ]
}

/// Rows as the lines of a schema written as one TOML string.
pub(crate) fn toml_lines(rows: &[(&'static str, String)]) -> String {
    rows.iter()
        .map(|(key, line)| format!("{key} = {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A component's own rows with the flag rows after them, as the pairs
/// `ComponentDef::schema` takes.
pub(crate) fn with_rows<'a>(
    own: &[(&'a str, &'a str)],
    overlay: &'a [(&'static str, String)],
) -> Vec<(&'a str, &'a str)> {
    own.iter()
        .copied()
        .chain(overlay.iter().map(|(key, line)| (*key, line.as_str())))
        .collect()
}

fn flag(params: &toml::Value, key: &str, fallback: bool) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

/// The 3D flags a component's params describe.
pub(crate) fn overlay_3d(params: &toml::Value) -> Overlay3d {
    let id = params
        .get(k::SEGMENTATION_ID)
        .and_then(as_f64)
        .unwrap_or(0.0)
        .max(0.0);
    Overlay3d {
        segmentation_id: u32::try_from(id as u64).unwrap_or(u32::MAX),
        receive_shadows: flag(params, k::RECEIVE_SHADOWS, true),
    }
}

/// The 3D flags read back into a component's `get`.
pub(crate) fn overlay_3d_to_map(overlay: Overlay3d, map: &mut toml::map::Map<String, toml::Value>) {
    map.insert(
        k::SEGMENTATION_ID.into(),
        toml::Value::Integer(i64::from(overlay.segmentation_id)),
    );
    map.insert(
        k::RECEIVE_SHADOWS.into(),
        toml::Value::Boolean(overlay.receive_shadows),
    );
}

/// Put a 3D node's flags and its debug drawing on it.
#[cfg(feature = "window")]
pub(crate) fn apply_3d(
    node: &mut kiss3d::scene::SceneNode3d,
    overlay: Overlay3d,
    debug: &DebugDraw,
) {
    crate::debug_draw::apply_3d(node, debug);
    node.set_receives_shadows(overlay.receive_shadows);
    if overlay.segmentation_id > 0 {
        node.apply_to_object_mut(&mut |object| object.set_segmentation_id(overlay.segmentation_id));
    }
}

/// The child that draws a node's wireframe and dots when one of Balaur's own
/// pipelines draws its surface: it shares the mesh and draws through kiss3d's
/// material, which is what draws lines and dots, with no surface of its own.
#[cfg(feature = "window")]
pub(crate) fn companion_3d(
    node: &mut kiss3d::scene::SceneNode3d,
    companion: &mut Option<kiss3d::scene::SceneNode3d>,
    overlay: Overlay3d,
    debug: &DebugDraw,
    pipeline: bool,
) {
    let wanted = pipeline && debug.draws_lines();
    if !wanted {
        if let Some(mut child) = companion.take() {
            child.remove();
        }
        return;
    }
    if companion.is_none() {
        let Some(mesh) = node.data().object().map(|object| object.mesh().clone()) else {
            return;
        };
        let mut child = node.add_mesh(mesh, glamx::Vec3::ONE);
        child
            .set_surface_rendering_activation(false)
            .set_casts_shadows(false);
        *companion = Some(child);
    }
    if let Some(child) = companion.as_mut() {
        apply_3d(child, overlay, &debug.lines_only());
    }
}

/// The companion follows its node's colour and scale, which a child's own
/// object does not inherit.
#[cfg(feature = "window")]
pub(crate) fn follow_3d(
    companion: Option<&mut kiss3d::scene::SceneNode3d>,
    color: kiss3d::color::Color,
    scale: glamx::Vec3,
) {
    if let Some(child) = companion {
        child
            .set_color(color)
            .set_local_scale(scale.x, scale.y, scale.z);
    }
}

/// The 2D twin of [`companion_3d`]: `mesh` is the geometry the node's own
/// pipeline draws, or `None` when kiss3d's material draws the node itself.
#[cfg(feature = "window")]
pub(crate) fn companion_2d(
    node: &mut kiss3d::scene::SceneNode2d,
    companion: &mut Option<kiss3d::scene::SceneNode2d>,
    debug: &DebugDraw,
    mesh: Option<std::rc::Rc<std::cell::RefCell<kiss3d::resource::GpuMesh2d>>>,
) {
    let Some(mesh) = mesh.filter(|_| debug.draws_lines()) else {
        if let Some(mut child) = companion.take() {
            child.detach();
        }
        return;
    };
    let child = companion.get_or_insert_with(|| node.add_mesh(mesh, glamx::Vec2::ONE));
    apply_2d(child, &Surface::default(), &debug.lines_only());
}

/// Put a 2D node's surface and its debug drawing on it and every piece under
/// it. A surface that leaves the blend to the texture takes the one each
/// piece's texture was uploaded for.
#[cfg(feature = "window")]
pub(crate) fn apply_2d(
    node: &mut kiss3d::scene::SceneNode2d,
    surface: &Surface,
    debug: &DebugDraw,
) {
    use kiss3d::scene::Blend2d;
    crate::debug_draw::apply_2d(node, debug);
    node.enable_backface_culling_recursive(!surface.double_sided_or(true));
    let asked = surface.blend_2d();
    node.apply_to_objects_mut_recursive(&mut |object| {
        let blend = asked.unwrap_or(if object.data().texture().premultiplied {
            Blend2d::PremultipliedAlpha
        } else {
            Blend2d::Alpha
        });
        object.set_blend(blend);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn the_flags_read_back_as_they_were_written() {
        let overlay = overlay_3d(&table("segmentation_id = 7\nreceive_shadows = false"));
        assert_eq!(overlay.segmentation_id, 7);
        let mut map = toml::map::Map::new();
        overlay_3d_to_map(overlay, &mut map);
        assert_eq!(overlay_3d(&toml::Value::Table(map)), overlay);
    }
}
