//! The `text2d` and `text3d` components: a block of text a scene node carries.
//!
//! Both write a [`TextRenderable`]; the backend mirrors it as one mesh over the
//! shaper's atlas, rebuilt when the node's `version` moves or the atlas grows.
//! Separate from `world_text`'s immediate calls, which keep nothing.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Enum, Written};
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::Bindings;

use crate::vocabulary::words;
use crate::world_text::{Align, Layers3d, Shaping, SpaceOptions, TextRenderable, TextStyle};
use balaur_core::layers::Layer;

/// Upright, italic, or the upright face slanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// Where a wrapped line may break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum LineBreak {
    WordOrGlyph,
    Word,
    Glyph,
}

/// Which part of a cut line the ellipsis stands in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Enum)]
pub enum TruncateAt {
    End,
    Start,
    Middle,
}

/// A block of `text` drawn in the 2D pass, `pixels_per_unit` font pixels per world unit.
#[derive(Component)]
#[component(name = "text2d")]
pub struct Text2d {
    /// The text drawn; `text_key` wins over it
    pub text: String,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// Height in font pixels, before pixels_per_unit sizes it in the world
    #[property(default = 32.0, min = 1.0)]
    pub font_size: f32,
    /// Where the block sits across the node's origin, and its lines within it: start and end follow the text's direction, left and right do not, justify stretches every full line
    #[property(default = Align::Center)]
    pub text_align: Align,
    /// The material every layer draws with, reading the glyph atlas as its texture; empty takes an inherited `material` component, else the built-in one. Its `[surface]` says whether the glyphs are cut out (`alpha = "mask"`), which sides of a `text3d` draw and whether the scene hides it
    #[property(asset = "material")]
    pub material: String,
    /// The face it draws in, and how: a face file such as `fonts/Inter.ttf`, or a `font` asset. Empty takes the theme's default font
    #[property(asset = "font", group = "font")]
    pub font: String,
    /// Stroke weight, 400 regular and 700 bold
    #[property(default = 400, min = 100, max = 900, group = "font")]
    pub font_weight: i64,
    /// Upright, the family's italic face, or the upright face slanted; a family with no italic is slanted either way
    #[property(default = FontStyle::Normal, group = "font")]
    pub font_style: FontStyle,
    /// How it is dressed past its font: line height and spacing, the lines drawn with the glyphs, an outline and a shadow. Empty draws it plain
    #[property(asset = "text_style", group = "font")]
    pub text_style: String,
    /// A key in the project's strings, re-read every frame so a language change shows at once
    #[property(group = "text")]
    pub text_key: String,
    /// Read the text as markup: bold, italic, colour, alignment, wave and inline images
    #[property(group = "text")]
    pub markup: bool,
    /// Font pixels the lines wrap at; zero runs the text on one line
    #[property(min = 0.0, group = "text")]
    pub max_width: f32,
    /// Font pixels to one world unit, sizing the block the way a sprite is sized
    #[property(default = 100.0, min = 0.01, group = "placement")]
    pub pixels_per_unit: f32,
    /// Font pixels the lines stop at; zero has no limit
    #[property(min = 0.0, group = "text")]
    pub max_height: f32,
    /// The most lines a block keeps; zero keeps every line
    #[property(min = 0, group = "text")]
    pub max_lines: i64,
    /// Where a wrapped line may break: between words with a too-long word cut anywhere, between words only, or anywhere
    #[property(default = LineBreak::WordOrGlyph, group = "text")]
    pub line_break: LineBreak,
    /// End a block cut short with an ellipsis; needs `max_width`, and cuts at `max_lines` or `max_height` when either is set, else at one line
    #[property(group = "text")]
    pub truncate: bool,
    /// Which part of a cut line the ellipsis stands in for
    #[property(default = TruncateAt::End, group = "text")]
    pub truncate_at: TruncateAt,
    /// Spaces between tab stops
    #[property(default = 8, min = 1, max = 64, group = "text")]
    pub tab_width: i64,
}

/// A block of `text` drawn in the 3D pass on a quad, `pixels_per_unit` font pixels per world unit; `billboard` turns it to the camera.
#[derive(Component)]
#[component(name = "text3d")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is its own property, not a state machine"
)]
pub struct Text3d {
    /// The text drawn; `text_key` wins over it
    pub text: String,
    /// Tint, as channel floats or #rrggbb / #rrggbbaa
    #[property(color, default = [1.0, 1.0, 1.0, 1.0])]
    pub color: [f32; 4],
    /// Height in font pixels, before pixels_per_unit sizes it in the world
    #[property(default = 32.0, min = 1.0)]
    pub font_size: f32,
    /// Where the block sits across the node's origin, and its lines within it: start and end follow the text's direction, left and right do not, justify stretches every full line
    #[property(default = Align::Center)]
    pub text_align: Align,
    /// The material every layer draws with, reading the glyph atlas as its texture; empty takes an inherited `material` component, else the built-in one. Its `[surface]` says whether the glyphs are cut out (`alpha = "mask"`), which sides of a `text3d` draw and whether the scene hides it
    #[property(asset = "material")]
    pub material: String,
    /// The face it draws in, and how: a face file such as `fonts/Inter.ttf`, or a `font` asset. Empty takes the theme's default font
    #[property(asset = "font", group = "font")]
    pub font: String,
    /// Stroke weight, 400 regular and 700 bold
    #[property(default = 400, min = 100, max = 900, group = "font")]
    pub font_weight: i64,
    /// Upright, the family's italic face, or the upright face slanted; a family with no italic is slanted either way
    #[property(default = FontStyle::Normal, group = "font")]
    pub font_style: FontStyle,
    /// How it is dressed past its font: line height and spacing, the lines drawn with the glyphs, an outline and a shadow. Empty draws it plain
    #[property(asset = "text_style", group = "font")]
    pub text_style: String,
    /// A key in the project's strings, re-read every frame so a language change shows at once
    #[property(group = "text")]
    pub text_key: String,
    /// Read the text as markup: bold, italic, colour, alignment, wave and inline images
    #[property(group = "text")]
    pub markup: bool,
    /// Font pixels the lines wrap at; zero runs the text on one line
    #[property(min = 0.0, group = "text")]
    pub max_width: f32,
    /// Font pixels to one world unit, sizing the block the way a sprite is sized
    #[property(default = 100.0, min = 0.01, group = "placement")]
    pub pixels_per_unit: f32,
    /// Font pixels the lines stop at; zero has no limit
    #[property(min = 0.0, group = "text")]
    pub max_height: f32,
    /// The most lines a block keeps; zero keeps every line
    #[property(min = 0, group = "text")]
    pub max_lines: i64,
    /// Where a wrapped line may break: between words with a too-long word cut anywhere, between words only, or anywhere
    #[property(default = LineBreak::WordOrGlyph, group = "text")]
    pub line_break: LineBreak,
    /// End a block cut short with an ellipsis; needs `max_width`, and cuts at `max_lines` or `max_height` when either is set, else at one line
    #[property(group = "text")]
    pub truncate: bool,
    /// Which part of a cut line the ellipsis stands in for
    #[property(default = TruncateAt::End, group = "text")]
    pub truncate_at: TruncateAt,
    /// Spaces between tab stops
    #[property(default = 8, min = 1, max = 64, group = "text")]
    pub tab_width: i64,
    /// Turn to face the camera every frame; off leaves it in the node's own plane
    #[property(default = true)]
    pub billboard: bool,
    /// Whether it casts a shadow from the lights that cast. Through a material whose surface cuts the glyphs out, the shadow keeps their shapes; otherwise each glyph shadows as its whole quad
    #[property(default = true, group = "shadows")]
    pub cast_shadow: bool,
    /// The layers this is drawn on: a `camera3d` draws it when their `render_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub render_layers: Vec<Layer>,
    /// The light layers this is lit on: a `light3d` lights it when their `light_layers` share one; empty is every layer
    #[property(flags, group = "layers")]
    pub light_layers: Vec<Layer>,
    /// Whether shadows land on it; off lights it as if nothing stood between it and every light
    #[property(default = true, group = "shadows")]
    pub receive_shadows: bool,
    /// The id `render.snap_aov("segmentation")` colours this node by; nodes sharing one share a colour. Zero takes one of its own, from 1 up
    #[property(min = 0, group = "layers")]
    pub segmentation_id: i64,
}

/// The renderable a `text2d` or a `text3d` describes, from the rows the two
/// share, every number held to what the shaper takes; `in_3d`, `in_space`
/// and `overlay_3d` are the 3D pass's.
macro_rules! renderable_of {
    ($asked:expr, $in_3d:expr, $in_space:expr, $overlay_3d:expr) => {{
        let asked = $asked;
        TextRenderable {
            text: asked.text.clone(),
            text_key: asked.text_key.clone(),
            style: TextStyle {
                size: asked.font_size.max(1.0),
                weight: u16::try_from(asked.font_weight.max(0)).unwrap_or(u16::MAX),
                color: asked.color,
                align: asked.text_align,
                markup: asked.markup,
                max_width: (asked.max_width > 0.0).then_some(asked.max_width),
                font: asked.font.clone(),
                text_style: asked.text_style.clone(),
                in_content: false,
                // A component's cutoff is its material's `[surface]`, read as
                // the block is drawn; this one is the immediate API's.
                alpha_cutoff: 0.0,
                shaping: Shaping {
                    font_style: asked.font_style.option().into(),
                    line_break: asked.line_break.option().into(),
                    truncate: asked.truncate,
                    truncate_at: asked.truncate_at.option().into(),
                    max_lines: u32::try_from(asked.max_lines.max(0)).unwrap_or(u32::MAX),
                    max_height: asked.max_height.max(0.0),
                    tab_width: u16::try_from(asked.tab_width.clamp(1, 64)).unwrap_or(8),
                },
            },
            pixels_per_unit: asked.pixels_per_unit.max(0.01),
            in_3d: $in_3d,
            in_space: $in_space,
            material: asked.material.clone(),
            overlay_3d: $overlay_3d,
            version: 0,
        }
    }};
}

/// Write the component, keeping the version so the backend rebuilds.
pub(crate) fn set_text(eng: &Engine, entity: Entity, mut next: TextRenderable) -> Result<()> {
    // Not a property: where the node sits says whose faces it draws in.
    next.style.in_content = balaur_core::project::in_content(eng, entity);
    let world = eng.world_mut();
    if let Ok(current) = world.get::<&TextRenderable>(entity) {
        if *current == next {
            return Ok(());
        }
        next.version = current.version.wrapping_add(1);
    }
    drop(world);
    eng.world_mut()
        .insert_one(entity, next)
        .map_err(|_| anyhow!("node is dead"))
}

/// A `text2d`'s renderable, laid out again from what was written.
fn text2d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let next = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Text2d>(entity) else {
            return Ok(());
        };
        renderable_of!(
            &*asked,
            false,
            SpaceOptions::default(),
            crate::overlay::Overlay3d::default()
        )
    };
    set_text(eng, entity, next)
}

/// A `text3d`'s renderable, laid out again from what was written.
fn text3d_written(eng: &Engine, entity: Entity, _written: Written) -> Result<()> {
    let next = {
        let world = eng.world();
        let Ok(asked) = world.get::<&Text3d>(entity) else {
            return Ok(());
        };
        let lighting = crate::Lighting3d::of(
            asked.cast_shadow,
            &asked.light_layers,
            &asked.render_layers,
            asked.receive_shadows,
            asked.segmentation_id,
        );
        let in_space = SpaceOptions {
            billboard: asked.billboard,
            layers: Layers3d {
                cast_shadow: lighting.shadows,
                light_layers: lighting.layers,
                render_layers: lighting.render_layers,
            },
        };
        renderable_of!(&*asked, true, in_space, lighting.overlay)
    };
    set_text(eng, entity, next)
}

/// `text2d`: a block of text in the 2D pass, sized like a sprite.
pub(crate) fn register_text2d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Text2d::NAME,
        ComponentDef {
            tags: &[words::ORTHOGRAPHIC, "render"],
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Text2d>(entity);
                let _ = world.remove_one::<TextRenderable>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<Text2d>(Some(text2d_written))
        },
    );
}

/// `text3d`: the same block in the 3D pass, facing the camera by default.
pub(crate) fn register_text3d_component(reg: &mut Registry<'_>) {
    reg.register_component(
        Text3d::NAME,
        ComponentDef {
            tags: &[words::PERSPECTIVE, "render"],
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<Text3d>(entity);
                let _ = world.remove_one::<TextRenderable>(entity);
                Ok(())
            }),
            ..ComponentDef::of::<Text3d>(Some(text3d_written))
        },
    );
}

/// `render.set_text` and `render.text`: the string a node draws, for a score
/// that changes every frame without rewriting the whole component.
pub(crate) fn install_text_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
}

/// The box a node's text was last laid out in, in its own space: what
/// `render.bounds` answers for a text, so it is picked like anything drawn.
/// The backend writes it when the block is shaped; a headless run has none.
pub(crate) struct LaidText {
    pub centre: glamx::Vec3,
    pub half: glamx::Vec3,
}

/// One node's mesh and what it was built from.
#[cfg(feature = "window")]
pub(crate) struct TextSlot {
    /// What holds the 2D layers: the block's one place in the 2D draw order.
    group_2d: Option<kiss3d::scene::SceneNode2d>,
    /// One node per layer: shadow, outline, then the text itself.
    two_d: Vec<kiss3d::scene::SceneNode2d>,
    three_d: Vec<kiss3d::scene::SceneNode3d>,
    /// Each layer's own colour, in the order of the nodes above: the shadow's,
    /// the outline's, a marked word's. An ancestor's tint multiplies them.
    colors: Vec<[f32; 4]>,
    version: u64,
    /// The size the glyphs were rasterised at. A camera that moves changes
    /// how many pixels the block covers, and past a bucket it is re-shaped
    /// rather than magnified.
    raster: f32,
    /// The string last shaped, so a `text_key` that resolves differently
    /// after a language change rebuilds without the component moving.
    shaped: String,
    /// The node's inherited material when this was built; a change rebuilds
    /// a block that names none of its own.
    inherited: balaur_core::scene::MaterialId,
    /// The asset generation it was built under: a saved font or style file
    /// rebuilds a block naming one.
    assets: u64,
}

/// The two material caches a block's layers draw through, 2D and 3D.
#[cfg(feature = "window")]
pub(crate) type TextMaterials<'a> = (
    &'a mut crate::shader_material::MaterialCache,
    &'a mut crate::shader_material_3d::MaterialCache3d,
);

#[cfg(feature = "window")]
impl TextSlot {
    /// Drop every node this slot made.
    fn detach(&mut self) {
        if let Some(mut group) = self.group_2d.take() {
            group.detach();
        }
        self.two_d.clear();
        for mut node in self.three_d.drain(..) {
            node.detach();
        }
    }

    /// The node the 2D order places, for a block in the 2D pass.
    pub(crate) fn group_2d(&self) -> Option<&kiss3d::scene::SceneNode2d> {
        self.group_2d.as_ref()
    }
}

/// Mirror every `text2d` and `text3d` node as a mesh over the shaper's atlas.
///
/// Rebuilt when the component's version moves, when a `text_key` resolves to
/// something new, or when the atlas has grown under it and the old UVs point
/// at glyphs that are no longer there.
#[cfg(feature = "window")]
pub(crate) fn sync_text(
    app: &balaur_core::App,
    (scene_2d, scene_3d): (
        &mut kiss3d::scene::SceneNode2d,
        &mut kiss3d::scene::SceneNode3d,
    ),
    (materials_2d, materials_3d): TextMaterials<'_>,
    slots: &mut std::collections::HashMap<Entity, TextSlot>,
    viewport_height: f32,
) {
    let (seen, wanted) = stale_blocks(app, slots, viewport_height);

    // Shape3d every block that moved before the atlas is uploaded, so one
    // upload covers the lot.
    let mut blocks = Vec::with_capacity(wanted.len());
    for (entity, text, resolved, raster, _) in &wanted {
        blocks.push(Some(crate::world_text::shape_at(
            &app.engine,
            resolved,
            &text.style,
            *raster,
        )));
        let _ = entity;
    }
    let texture = crate::world_text::atlas_texture(&app.engine);

    for ((entity, text, resolved, raster, from_parent), block) in wanted.into_iter().zip(blocks) {
        if let Some(mut old) = slots.remove(&entity) {
            old.detach();
        }
        let debug = crate::debug_draw::DebugDraw::of(&app.engine.world(), entity);
        let Some(block) = block else { continue };
        let Some(texture) = texture.clone() else {
            continue;
        };
        // Shaped at `raster`, wanted at `font_size`: the block is scaled back.
        let scale = (text.style.size / raster) / text.pixels_per_unit;
        let (centre, half) = crate::world_text::laid_box(&block, scale, text.style.align);
        let _ = app.engine.world_mut().insert_one(
            entity,
            LaidText {
                centre: centre.extend(0.0),
                half: half.extend(0.0),
            },
        );
        let (custom_2d, custom_3d) =
            materials_of(app, &text, from_parent, (materials_2d, materials_3d));
        // Which sides draw, the cutoff and the depth test: the material's
        // `[surface]`, text drawing both sides where it says nothing.
        let surface =
            crate::material::flat_surface_of(&app.engine, &material_reference(&text, from_parent));
        let mut slot = TextSlot {
            group_2d: None,
            two_d: Vec::new(),
            three_d: Vec::new(),
            colors: Vec::new(),
            version: text.version,
            raster,
            shaped: resolved,
            inherited: from_parent,
            assets: balaur_core::assets::generation(&app.engine),
        };
        // Shadow, outline and text: a node each, drawn in that order.
        let look = balaur_text::text_style::of(&app.engine, &text.style.text_style);
        for (layer, (shifts, [r, g, b, a], picks)) in
            crate::world_text::layers(&block, &text.style, &look)
                .into_iter()
                .enumerate()
        {
            let tint = kiss3d::color::Color::new(r, g, b, a);
            if text.in_3d {
                if let Some(mesh) = crate::world_text::mesh_3d(
                    &block,
                    scale,
                    text.style.align,
                    &shifts,
                    &picks,
                    crate::world_text::depth_of(layer),
                ) {
                    let mut node = scene_3d.add_mesh(mesh, glamx::Vec3::ONE);
                    node.set_texture(texture.clone());
                    node.enable_backface_culling(!surface.double_sided_or(true));
                    node.set_color(tint);
                    let layers = text.in_space.layers;
                    node.set_casts_shadows(layers.cast_shadow)
                        .set_light_layers(layers.light_layers)
                        .set_render_layers(layers.render_layers);
                    crate::world_text::mask_3d(&mut node, surface.cutoff());
                    node.set_depth_test(surface.depth_test);
                    crate::overlay::apply_3d(&mut node, text.overlay_3d, &debug);
                    if let Some(material) = custom_3d.clone() {
                        node.set_material(material);
                    }
                    slot.three_d.push(node);
                    slot.colors.push([r, g, b, a]);
                }
            } else if let Some(mesh) =
                crate::world_text::mesh_2d(&block, scale, text.style.align, &shifts, &picks)
            {
                let group = slot.group_2d.get_or_insert_with(|| scene_2d.add_group());
                let mut node = group.add_mesh(mesh, glamx::Vec2::ONE);
                node.set_texture(texture.clone());
                node.set_color(tint);
                crate::world_text::mask_2d(&mut node, surface.cutoff());
                crate::overlay::apply_2d(&mut node, &surface, &debug);
                if let Some(material) = custom_2d.clone() {
                    node.set_material(material);
                }
                slot.two_d.push(node);
                slot.colors.push([r, g, b, a]);
            }
        }
        slots.insert(entity, slot);
    }

    place(app, slots, &seen);
}

/// A block to shape again: its text as resolved, the size it rasterises at,
/// and the material it inherits.
#[cfg(feature = "window")]
type Stale = (
    Entity,
    TextRenderable,
    String,
    f32,
    balaur_core::scene::MaterialId,
);

/// Every text node, and the ones whose block has to be built again.
#[cfg(feature = "window")]
fn stale_blocks(
    app: &balaur_core::App,
    slots: &std::collections::HashMap<Entity, TextSlot>,
    viewport_height: f32,
) -> (std::collections::HashSet<Entity>, Vec<Stale>) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    let world = app.engine.world();
    let generation = balaur_core::assets::generation(&app.engine);
    let mut seen = std::collections::HashSet::new();
    let mut wanted = Vec::new();
    for (entity, text, global) in &mut world.query::<(Entity, &TextRenderable, &GlobalTransform)>()
    {
        seen.insert(entity);
        let resolved = text.resolved(&app.engine);
        let raster = raster_size(app, text, global, viewport_height);
        let from_parent = world
            .get::<&GlobalAppearance>(entity)
            .map_or_else(|_| GlobalAppearance::identity().material, |a| a.material);
        let rebuild = slots.get(&entity).is_none_or(|slot| {
            slot.version != text.version
                || slot.shaped != resolved
                || (slot.raster - raster).abs() > f32::EPSILON
                || (text.material.is_empty() && slot.inherited != from_parent)
                || (slot.assets != generation && names_a_file(text))
        });
        if rebuild {
            wanted.push((entity, text.clone(), resolved, raster, from_parent));
        }
    }
    (seen, wanted)
}

/// Whether a block names a font or a style kept in a file, which a save can
/// change under it.
#[cfg(feature = "window")]
fn names_a_file(text: &TextRenderable) -> bool {
    [&text.style.font, &text.style.text_style]
        .into_iter()
        .any(|reference| balaur_text::text_style::names_a_file(reference))
}

/// The material a block draws with: its own, else the one it inherited.
#[cfg(feature = "window")]
fn material_reference(
    text: &TextRenderable,
    from_parent: balaur_core::scene::MaterialId,
) -> String {
    if text.material.is_empty() {
        from_parent.reference().to_string()
    } else {
        text.material.clone()
    }
}

/// The material a block's layers draw with, its own or the one it inherited,
/// in the dimension it draws in.
#[cfg(feature = "window")]
fn materials_of(
    app: &balaur_core::App,
    text: &TextRenderable,
    from_parent: balaur_core::scene::MaterialId,
    (materials_2d, materials_3d): (
        &mut crate::shader_material::MaterialCache,
        &mut crate::shader_material_3d::MaterialCache3d,
    ),
) -> (
    Option<crate::shader_material::SharedMaterial>,
    Option<crate::shader_material_3d::Shared3d>,
) {
    let reference = material_reference(text, from_parent);
    if text.in_3d {
        (None, materials_3d.for_node(app, &reference, ""))
    } else {
        (materials_2d.for_node(app, &reference, ""), None)
    }
}

/// Put every live block where its node is, and drop the ones whose node has
/// gone. Split from the rebuild above: one walks what changed, this walks all.
#[cfg(feature = "window")]
fn place(
    app: &balaur_core::App,
    slots: &mut std::collections::HashMap<Entity, TextSlot>,
    seen: &std::collections::HashSet<Entity>,
) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    // Where the eye is, for the blocks that face it.
    let eye = app
        .engine
        .try_resource::<crate::ViewportSnapshot3d>()
        .map(|snapshot| {
            let e = snapshot.borrow().eye;
            glamx::Vec3::new(e[0], e[1], e[2])
        });

    // Place what survives, and drop what the scene no longer holds.
    let world = app.engine.world();
    slots.retain(|entity, slot| {
        if !seen.contains(entity) {
            slot.detach();
            return false;
        }
        let Ok(global) = world.get::<&GlobalTransform>(*entity) else {
            return true;
        };
        let Ok(text) = world.get::<&TextRenderable>(*entity) else {
            return true;
        };
        let appearance = world
            .get::<&GlobalAppearance>(*entity)
            .map_or_else(|_| GlobalAppearance::identity(), |a| *a);
        let visible = appearance.visible;
        // Per frame rather than at build: the block is rebuilt only when the
        // text or its layout changes, and an ancestor's tint moves every tick.
        let tinted = |layer: usize| {
            let own = slot.colors.get(layer).copied().unwrap_or(text.style.color);
            let [r, g, b, a] = crate::sync_2d::modulate(own, appearance.tint.to_array());
            kiss3d::color::Color::new(r, g, b, a)
        };
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        for (layer, node) in slot.two_d.iter_mut().enumerate() {
            node.set_position(glamx::Vec2::new(global.position.x, global.position.y));
            node.set_rotation(angle);
            node.set_local_scale(global.scale.x, global.scale.y);
            node.set_visible(visible);
            node.set_color(tinted(layer));
        }
        // A billboard turns to the eye every frame; otherwise the block sits
        // in the node's own plane, like a sign painted on a wall.
        let turned = if text.in_space.billboard {
            eye.map_or(global.rotation, |eye| facing(eye - global.position))
        } else {
            global.rotation
        };
        for (layer, node) in slot.three_d.iter_mut().enumerate() {
            node.set_position(global.position);
            node.set_local_scale(global.scale.x, global.scale.y, global.scale.z);
            node.set_visible(visible);
            node.set_rotation(turned);
            node.set_color(tinted(layer));
        }
        true
    });
}

/// The rotation that turns a quad's +z along `towards`, keeping its up as
/// close to the world's as it can — what a billboard needs.
#[cfg(feature = "window")]
fn facing(towards: glamx::Vec3) -> glamx::Quat {
    let forward = towards.normalize_or_zero();
    if forward.length_squared() < 0.5 {
        return glamx::Quat::IDENTITY;
    }
    // Straight above or below, the world's up is no help; fall back to -z.
    let reference = if forward.y.abs() > 0.999 {
        glamx::Vec3::new(0.0, 0.0, -1.0)
    } else {
        glamx::Vec3::Y
    };
    let right = reference.cross(forward).normalize_or_zero();
    let up = forward.cross(right);
    glamx::Quat::from_mat3(&glamx::Mat3::from_cols(right, up, forward))
}

/// One em in world units: the font size over `pixels_per_unit`, grown or
/// shrunk by the node's own scale and every ancestor's.
#[cfg(feature = "window")]
fn em_in_world(size: f32, pixels_per_unit: f32, scale: glamx::Vec3) -> f32 {
    size / pixels_per_unit.max(0.01) * scale.x.abs().max(scale.y.abs())
}

/// The size a block's glyphs should be rasterised at: how many pixels one em
/// covers on screen, in buckets so a moving camera re-shapes rarely.
///
/// Falls back to the asked size when nothing has published a camera yet.
#[cfg(feature = "window")]
fn raster_size(
    app: &balaur_core::App,
    text: &TextRenderable,
    global: &balaur_core::GlobalTransform,
    viewport_height: f32,
) -> f32 {
    let em_world = em_in_world(text.style.size, text.pixels_per_unit, global.scale);
    let per_unit = if text.in_3d {
        let Some(snapshot) = app.engine.try_resource::<crate::ViewportSnapshot3d>() else {
            return balaur_text::bucket(text.style.size);
        };
        let snapshot = snapshot.borrow();
        let eye = glamx::Vec3::new(snapshot.eye[0], snapshot.eye[1], snapshot.eye[2]);
        let distance = (eye - global.position).length().max(0.01);
        // Half the frustum's height at that distance is what fills half the
        // viewport, so this is pixels to the world unit.
        let half = libm::tanf(snapshot.fov / 2.0).max(1e-4) * distance;
        viewport_height / (2.0 * half)
    } else {
        let Some(snapshot) = app.engine.try_resource::<crate::ViewportSnapshot2d>() else {
            return balaur_text::bucket(text.style.size);
        };
        // The 2D camera's zoom is already pixels to the world unit.
        snapshot.borrow().zoom.max(0.01)
    };
    balaur_text::bucket((em_world * per_unit).clamp(1.0, 512.0))
}

#[cfg(all(test, feature = "window"))]
mod tests {
    /// Text under a scaled parent is that much larger or smaller, as a
    /// sprite is, whichever axis is flipped.
    #[test]
    fn a_scaled_node_s_em_is_scaled_with_it() {
        let em = |x, y| super::em_in_world(280.0, 100.0, glamx::Vec3::new(x, y, 1.0));
        assert!((em(1.0, 1.0) - 2.8).abs() < 1e-5);
        assert!((em(0.118, 0.118) - 0.3304).abs() < 1e-4);
        assert!((em(-2.0, 1.0) - 5.6).abs() < 1e-5);
    }
}
