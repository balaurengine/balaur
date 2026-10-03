//! What a drawn node shows beside its surface, and the flags that are the
//! node's rather than its material's: a wireframe over every triangle edge,
//! a dot on every vertex, whether the surface itself draws, and in 3D the
//! segmentation id and whether shadows land on it; in 2D how it blends and
//! whether it culls.
//!
//! The keys are the same on every renderable of a dimension, so their schema
//! lines, reader and read-back are written once here and each component
//! appends them. kiss3d's built-in materials draw all of them; a node drawn
//! through one of Balaur's own pipelines does not yet, and each line says so.

use balaur_core::components::as_f64;

use crate::vocabulary::{keys as k, options, words};

/// Whether a wireframe's width or a vertex's size scales with the view or
/// stays in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sizing {
    #[default]
    World,
    Screen,
}

impl Sizing {
    fn of(word: &str) -> Self {
        if word == words::SCREEN {
            Self::Screen
        } else {
            Self::World
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::World => words::WORLD,
            Self::Screen => words::SCREEN,
        }
    }

    /// kiss3d's `use_perspective` flag.
    #[cfg(feature = "window")]
    fn perspective(self) -> bool {
        self == Self::World
    }
}

/// How a 2D surface lands on what is under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlendMode {
    /// Straight alpha, or premultiplied for a texture uploaded that way.
    #[default]
    Auto,
    Alpha,
    Premultiplied,
    Add,
    Multiply,
    Screen,
    Opaque,
}

impl BlendMode {
    const WORDS: [(Self, &'static str); 7] = [
        (Self::Auto, words::AUTO),
        (Self::Alpha, words::ALPHA),
        (Self::Premultiplied, words::PREMULTIPLIED),
        (Self::Add, words::ADD),
        (Self::Multiply, words::MULTIPLY),
        (Self::Screen, words::SCREEN),
        (Self::Opaque, words::OPAQUE),
    ];

    fn of(word: &str) -> Option<Self> {
        Self::WORDS
            .iter()
            .find(|(_, name)| *name == word)
            .map(|(mode, _)| *mode)
    }

    fn word(self) -> &'static str {
        Self::WORDS
            .iter()
            .find(|(mode, _)| *mode == self)
            .map_or(words::AUTO, |(_, name)| name)
    }

    /// kiss3d's blend for this mode; `None` leaves what the texture set.
    #[cfg(feature = "window")]
    pub(crate) fn blend_2d(self) -> Option<kiss3d::scene::Blend2d> {
        use kiss3d::scene::Blend2d;
        Some(match self {
            Self::Auto => return None,
            Self::Alpha => Blend2d::Alpha,
            Self::Premultiplied => Blend2d::PremultipliedAlpha,
            Self::Add => Blend2d::Additive,
            Self::Multiply => Blend2d::Multiply,
            Self::Screen => Blend2d::Screen,
            Self::Opaque => Blend2d::Opaque,
        })
    }
}

/// The overlay keys every 3D renderable takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay3d {
    /// Zero draws no wireframe.
    pub wireframe_width: f32,
    pub wireframe_sizing: Sizing,
    /// An alpha of zero takes the node's own colour.
    pub wireframe_color: [f32; 4],
    /// Zero draws no vertices.
    pub dot_size: f32,
    pub dot_sizing: Sizing,
    pub dot_color: [f32; 4],
    pub draw_surface: bool,
    /// Zero keeps the id kiss3d hands every object.
    pub segmentation_id: u32,
    pub receive_shadows: bool,
    /// Off draws the node over what is already there, leaving the depth alone.
    pub depth_test: bool,
}

impl Default for Overlay3d {
    fn default() -> Self {
        Self {
            wireframe_width: 0.0,
            wireframe_sizing: Sizing::World,
            wireframe_color: [0.0; 4],
            dot_size: 0.0,
            dot_sizing: Sizing::World,
            dot_color: [0.0; 4],
            draw_surface: true,
            segmentation_id: 0,
            receive_shadows: true,
            depth_test: true,
        }
    }
}

/// The overlay keys every 2D drawable takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay2d {
    pub blend: BlendMode,
    pub wireframe_width: f32,
    pub wireframe_sizing: Sizing,
    pub wireframe_color: [f32; 4],
    pub dot_size: f32,
    pub dot_sizing: Sizing,
    pub dot_color: [f32; 4],
    pub draw_surface: bool,
    pub cull_back_faces: bool,
}

impl Default for Overlay2d {
    fn default() -> Self {
        Self {
            blend: BlendMode::Auto,
            wireframe_width: 0.0,
            wireframe_sizing: Sizing::World,
            wireframe_color: [1.0; 4],
            dot_size: 0.0,
            dot_sizing: Sizing::World,
            dot_color: [1.0; 4],
            draw_surface: true,
            cull_back_faces: false,
        }
    }
}

/// Who draws a node, for the words a schema line adds about what is not
/// honoured yet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drawn {
    /// kiss3d's own material, unless the node names a `material`.
    Builtin,
    /// One of Balaur's own pipelines, whatever the node names.
    Pipeline,
    /// A `text2d`, which a cutoff also moves onto a Balaur pipeline.
    Text2d,
    /// A `text3d`, whose glyph quads are drawn as several layer nodes.
    Text3d,
}

impl Drawn {
    fn not_yet(self) -> &'static str {
        match self {
            Self::Builtin | Self::Text2d => {
                "Not drawn on a skinned mesh, which Balaur poses in its own vertex stage"
            }
            Self::Text3d => {
                "Not drawn on a block with a `material`, whose layers draw through Balaur's own pipeline"
            }
            Self::Pipeline => {
                "Not drawn yet: this component draws through Balaur's own pipeline, which does not read it"
            }
        }
    }

    fn not_yet_2d(self) -> &'static str {
        match self {
            Self::Builtin | Self::Text3d => "",
            Self::Text2d => {
                ". Not drawn with `alpha_cutoff` above zero or a material, whose layers draw through Balaur's own pipeline"
            }
            Self::Pipeline => {
                ". Not drawn on a skinned polygon, which a rig poses in Balaur's own vertex stage"
            }
        }
    }
}

/// The sizing row: kiss3d's 3D `world` is pixels at one unit from the
/// camera, divided by depth, where its 2D one is world units.
fn sizing_line(what: &str, flat: bool) -> String {
    let world = if flat {
        "world units, which the camera's zoom scales"
    } else {
        "pixels at one world unit from the camera, thinning with distance"
    };
    format!(
        r#"{{ type = "enum", default = "{}", options = [{}], description = "What `{what}` counts: `world` is {world}, `screen` is pixels" }}"#,
        words::WORLD,
        options(words::SIZINGS)
    )
}

/// The overlay rows a 3D renderable appends to its schema.
pub(crate) fn schema_3d(drawn: Drawn) -> Vec<(&'static str, String)> {
    let not_yet = drawn.not_yet();
    vec![
        (k::WIREFRAME_WIDTH, format!(r#"{{ type = "float", default = 0.0, min = 0.0, description = "Width of a line drawn along every triangle edge; zero draws none. {not_yet}. Not on WebGL2" }}"#)),
        (k::WIREFRAME_SIZING, sizing_line(k::WIREFRAME_WIDTH, false)),
        (k::WIREFRAME_COLOR, format!(r#"{{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The wireframe's colour; an alpha of zero takes the node's own. {not_yet}" }}"#)),
        (k::DOT_SIZE, format!(r#"{{ type = "float", default = 0.0, min = 0.0, description = "Size of a dot drawn on every vertex; zero draws none. {not_yet}. Not on WebGL2" }}"#)),
        (k::DOT_SIZING, sizing_line(k::DOT_SIZE, false)),
        (k::DOT_COLOR, format!(r#"{{ type = "color", default = [0.0, 0.0, 0.0, 0.0], description = "The vertex dots' colour; an alpha of zero takes the node's own. {not_yet}" }}"#)),
        (k::DRAW_SURFACE, r#"{ type = "bool", default = true, description = "Whether the surface draws; off leaves the wireframe and the dots alone" }"#.to_string()),
        (k::SEGMENTATION_ID, r#"{ type = "int", default = 0, min = 0, description = "The id `render.snap_aov(\"segmentation\")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up" }"#.to_string()),
        (k::RECEIVE_SHADOWS, r#"{ type = "bool", default = true, description = "Whether shadows land on it; off lights it as if nothing stood between it and every light" }"#.to_string()),
        (k::DEPTH_TEST, r#"{ type = "bool", default = true, description = "Let the scene hide it; off draws it over everything drawn before it and leaves the depth as it was" }"#.to_string()),
    ]
}

/// The overlay rows a 2D drawable appends to its schema.
pub(crate) fn schema_2d(drawn: Drawn) -> Vec<(&'static str, String)> {
    let not_yet = drawn.not_yet_2d();
    vec![
        (
            k::BLEND_MODE,
            format!(
                r#"{{ type = "enum", default = "{}", options = [{}], description = "How the surface lands on what is under it: straight alpha, a colour that already carries its alpha, added light, multiplied shade, screen, or opaque. `auto` is premultiplied for a texture uploaded with `premultiply`, else alpha" }}"#,
                words::AUTO,
                options(words::BLEND_MODES)
            ),
        ),
        (
            k::WIREFRAME_WIDTH,
            format!(
                r#"{{ type = "float", default = 0.0, min = 0.0, description = "Width of a line drawn along every triangle edge, not the outline; zero draws none{not_yet}" }}"#
            ),
        ),
        (k::WIREFRAME_SIZING, sizing_line(k::WIREFRAME_WIDTH, true)),
        (
            k::WIREFRAME_COLOR,
            format!(
                r#"{{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "The wireframe's colour{not_yet}" }}"#
            ),
        ),
        (
            k::DOT_SIZE,
            format!(
                r#"{{ type = "float", default = 0.0, min = 0.0, description = "Size of a dot drawn on every vertex; zero draws none{not_yet}" }}"#
            ),
        ),
        (k::DOT_SIZING, sizing_line(k::DOT_SIZE, true)),
        (
            k::DOT_COLOR,
            format!(
                r#"{{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "The dots' colour{not_yet}" }}"#
            ),
        ),
        (
            k::DRAW_SURFACE,
            r#"{ type = "bool", default = true, description = "Whether the surface draws; off leaves the wireframe and the dots alone" }"#.to_string(),
        ),
        (
            k::CULL_BACK_FACES,
            r#"{ type = "bool", default = false, description = "Skip a triangle whose back faces the viewer, as a negative scale turns one" }"#.to_string(),
        ),
    ]
}

/// Rows as the lines of a schema written as one TOML string.
pub(crate) fn toml_lines(rows: &[(&'static str, String)]) -> String {
    rows.iter()
        .map(|(key, line)| format!("{key} = {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A component's own rows with the overlay rows after them, as the pairs
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

fn number(params: &toml::Value, key: &str, fallback: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(fallback, |v| v as f32)
}

fn flag(params: &toml::Value, key: &str, fallback: bool) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

fn sizing(params: &toml::Value, key: &str) -> Sizing {
    Sizing::of(
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default(),
    )
}

/// The 3D overlay a component's params describe.
pub(crate) fn overlay_3d(params: &toml::Value) -> Overlay3d {
    let d = Overlay3d::default();
    Overlay3d {
        wireframe_width: number(params, k::WIREFRAME_WIDTH, 0.0).max(0.0),
        wireframe_sizing: sizing(params, k::WIREFRAME_SIZING),
        wireframe_color: crate::color_from_key(params, k::WIREFRAME_COLOR, d.wireframe_color),
        dot_size: number(params, k::DOT_SIZE, 0.0).max(0.0),
        dot_sizing: sizing(params, k::DOT_SIZING),
        dot_color: crate::color_from_key(params, k::DOT_COLOR, d.dot_color),
        draw_surface: flag(params, k::DRAW_SURFACE, true),
        segmentation_id: u32::try_from(number(params, k::SEGMENTATION_ID, 0.0).max(0.0) as u64)
            .unwrap_or(u32::MAX),
        receive_shadows: flag(params, k::RECEIVE_SHADOWS, true),
        depth_test: flag(params, k::DEPTH_TEST, true),
    }
}

/// The 2D overlay a component's params describe.
///
/// # Errors
/// A `blend_mode` that is none of the words.
pub(crate) fn overlay_2d(params: &toml::Value) -> anyhow::Result<Overlay2d> {
    let d = Overlay2d::default();
    let word = params
        .get(k::BLEND_MODE)
        .and_then(toml::Value::as_str)
        .unwrap_or(words::AUTO);
    let blend = BlendMode::of(word).ok_or_else(|| {
        anyhow::anyhow!(
            "`{}` is one of {}, not `{word}`",
            k::BLEND_MODE,
            words::BLEND_MODES.join(", ")
        )
    })?;
    Ok(Overlay2d {
        blend,
        wireframe_width: number(params, k::WIREFRAME_WIDTH, 0.0).max(0.0),
        wireframe_sizing: sizing(params, k::WIREFRAME_SIZING),
        wireframe_color: crate::color_from_key(params, k::WIREFRAME_COLOR, d.wireframe_color),
        dot_size: number(params, k::DOT_SIZE, 0.0).max(0.0),
        dot_sizing: sizing(params, k::DOT_SIZING),
        dot_color: crate::color_from_key(params, k::DOT_COLOR, d.dot_color),
        draw_surface: flag(params, k::DRAW_SURFACE, true),
        cull_back_faces: flag(params, k::CULL_BACK_FACES, false),
    })
}

fn float(value: f32) -> toml::Value {
    toml::Value::Float(f64::from(value))
}

fn word(value: &str) -> toml::Value {
    toml::Value::String(value.to_string())
}

/// The 3D overlay read back into a component's `get`.
pub(crate) fn overlay_3d_to_map(
    overlay: &Overlay3d,
    map: &mut toml::map::Map<String, toml::Value>,
) {
    map.insert(k::WIREFRAME_WIDTH.into(), float(overlay.wireframe_width));
    map.insert(
        k::WIREFRAME_SIZING.into(),
        word(overlay.wireframe_sizing.word()),
    );
    map.insert(
        k::WIREFRAME_COLOR.into(),
        crate::color_to_toml(overlay.wireframe_color),
    );
    map.insert(k::DOT_SIZE.into(), float(overlay.dot_size));
    map.insert(k::DOT_SIZING.into(), word(overlay.dot_sizing.word()));
    map.insert(k::DOT_COLOR.into(), crate::color_to_toml(overlay.dot_color));
    map.insert(
        k::DRAW_SURFACE.into(),
        toml::Value::Boolean(overlay.draw_surface),
    );
    map.insert(
        k::SEGMENTATION_ID.into(),
        toml::Value::Integer(i64::from(overlay.segmentation_id)),
    );
    map.insert(
        k::RECEIVE_SHADOWS.into(),
        toml::Value::Boolean(overlay.receive_shadows),
    );
    map.insert(
        k::DEPTH_TEST.into(),
        toml::Value::Boolean(overlay.depth_test),
    );
}

/// The 2D overlay read back into a component's `get`.
pub(crate) fn overlay_2d_to_map(
    overlay: &Overlay2d,
    map: &mut toml::map::Map<String, toml::Value>,
) {
    map.insert(k::BLEND_MODE.into(), word(overlay.blend.word()));
    map.insert(k::WIREFRAME_WIDTH.into(), float(overlay.wireframe_width));
    map.insert(
        k::WIREFRAME_SIZING.into(),
        word(overlay.wireframe_sizing.word()),
    );
    map.insert(
        k::WIREFRAME_COLOR.into(),
        crate::color_to_toml(overlay.wireframe_color),
    );
    map.insert(k::DOT_SIZE.into(), float(overlay.dot_size));
    map.insert(k::DOT_SIZING.into(), word(overlay.dot_sizing.word()));
    map.insert(k::DOT_COLOR.into(), crate::color_to_toml(overlay.dot_color));
    map.insert(
        k::DRAW_SURFACE.into(),
        toml::Value::Boolean(overlay.draw_surface),
    );
    map.insert(
        k::CULL_BACK_FACES.into(),
        toml::Value::Boolean(overlay.cull_back_faces),
    );
}

/// A colour with no alpha is kiss3d's "take the object's own".
#[cfg(feature = "window")]
fn color_or_object(color: [f32; 4]) -> Option<kiss3d::color::Color> {
    let [r, g, b, a] = color;
    (a > 0.0).then(|| kiss3d::color::Color::new(r, g, b, a))
}

/// Put a 3D overlay on a node.
#[cfg(feature = "window")]
pub(crate) fn apply_3d(node: &mut kiss3d::scene::SceneNode3d, overlay: &Overlay3d) {
    node.set_lines_width(
        overlay.wireframe_width,
        overlay.wireframe_sizing.perspective(),
    );
    node.set_lines_color(color_or_object(overlay.wireframe_color));
    node.set_points_size(overlay.dot_size, overlay.dot_sizing.perspective());
    node.set_points_color(color_or_object(overlay.dot_color));
    node.set_surface_rendering_activation(overlay.draw_surface);
    node.set_receives_shadows(overlay.receive_shadows);
    node.set_depth_test(overlay.depth_test);
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
    overlay: &Overlay3d,
    pipeline: bool,
) {
    let wanted = pipeline && (overlay.wireframe_width > 0.0 || overlay.dot_size > 0.0);
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
        let mut lines = *overlay;
        lines.draw_surface = false;
        apply_3d(child, &lines);
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
    overlay: &Overlay2d,
    mesh: Option<std::rc::Rc<std::cell::RefCell<kiss3d::resource::GpuMesh2d>>>,
) {
    let wanted = overlay.wireframe_width > 0.0 || overlay.dot_size > 0.0;
    let Some(mesh) = mesh.filter(|_| wanted) else {
        if let Some(mut child) = companion.take() {
            child.detach();
        }
        return;
    };
    let child = companion.get_or_insert_with(|| node.add_mesh(mesh, glamx::Vec2::ONE));
    let mut lines = *overlay;
    lines.draw_surface = false;
    lines.blend = BlendMode::Auto;
    apply_2d(child, &lines);
}

/// Put a 2D overlay on a node and every piece under it. `Auto` takes the
/// blend each piece's texture was uploaded for.
#[cfg(feature = "window")]
pub(crate) fn apply_2d(node: &mut kiss3d::scene::SceneNode2d, overlay: &Overlay2d) {
    use kiss3d::scene::Blend2d;
    let [r, g, b, a] = overlay.wireframe_color;
    let [vr, vg, vb, va] = overlay.dot_color;
    node.set_lines_width_recursive(
        overlay.wireframe_width,
        overlay.wireframe_sizing.perspective(),
    );
    node.set_lines_color_recursive(Some(kiss3d::color::Color::new(r, g, b, a)));
    node.set_points_size_recursive(overlay.dot_size, overlay.dot_sizing.perspective());
    node.set_points_color_recursive(Some(kiss3d::color::Color::new(vr, vg, vb, va)));
    node.set_surface_rendering_activation_recursive(overlay.draw_surface);
    node.enable_backface_culling_recursive(overlay.cull_back_faces);
    let asked = overlay.blend.blend_2d();
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
    fn an_overlay_reads_back_as_it_was_written() {
        let params = table(
            "wireframe_width = 2.0\nwireframe_sizing = \"screen\"\nwireframe_color = [1.0, 0.0, 0.0, 1.0]\ndot_size = 4.0\ndraw_surface = false\nsegmentation_id = 7\nreceive_shadows = false",
        );
        let overlay = overlay_3d(&params);
        assert_eq!(overlay.wireframe_sizing, Sizing::Screen);
        assert_eq!(overlay.segmentation_id, 7);
        let mut map = toml::map::Map::new();
        overlay_3d_to_map(&overlay, &mut map);
        assert_eq!(overlay_3d(&toml::Value::Table(map)), overlay);
    }

    #[test]
    fn every_blend_word_reads_back_as_itself() {
        for word in words::BLEND_MODES {
            let overlay = overlay_2d(&table(&format!("blend_mode = \"{word}\""))).unwrap();
            assert_eq!(overlay.blend.word(), *word);
        }
        assert!(overlay_2d(&table("blend_mode = \"lighten\"")).is_err());
    }
}
