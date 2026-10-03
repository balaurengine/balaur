//! The `text2d` and `text3d` components: a block of text a scene node carries.
//!
//! Both write a [`TextRenderable`]; the backend mirrors it as one mesh over the
//! shaper's atlas, rebuilt when the node's `version` moves or the atlas grows.
//! Separate from `world_text`'s immediate calls, which keep nothing.

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;
use balaur_script::Bindings;

use crate::vocabulary::keys as k;
use crate::vocabulary::words;
use crate::world_text::{Align, TextRenderable, TextStyle};

/// Write the component, keeping the version so the backend rebuilds.
pub(crate) fn set_text(eng: &Engine, entity: Entity, mut next: TextRenderable) -> Result<()> {
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

/// Every key both components share, as schema lines.
fn shared_schema() -> Vec<(&'static str, String)> {
    vec![
        (k::TEXT, r#"{ type = "string", default = "", description = "The text drawn; `text_key` wins over it" }"#.into()),
        (k::TEXT_KEY, r#"{ type = "string", default = "", description = "A key in the project's strings, re-read every frame so a language change shows at once" }"#.into()),
        (k::FONT_SIZE, r#"{ type = "float", default = 32.0, min = 1.0, description = "Height in font pixels, before pixels_per_unit sizes it in the world" }"#.into()),
        (k::FONT_WEIGHT, r#"{ type = "int", default = 400, min = 100, max = 900, description = "Stroke weight, 400 regular and 700 bold" }"#.into()),
        (k::FONT_STYLE, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Upright, the family's italic face, or the upright face slanted; a family with no italic is slanted either way" }}"#, words::NORMAL, crate::vocabulary::options(words::FONT_STYLES))),
        (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#.into()),
        (k::TEXT_ALIGN, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where the block sits across the node's origin, and its lines within it: start and end follow the text's direction, left and right do not, justify stretches every full line" }}"#, words::CENTER, crate::vocabulary::options(words::TEXT_ALIGNS))),
        (k::MAX_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Font pixels the lines wrap at; zero runs the text on one line" }"#.into()),
        (k::MARKUP, r#"{ type = "bool", default = false, description = "Read the text as markup: bold, italic, colour, alignment, wave and inline images" }"#.into()),
        (k::PIXELS_PER_UNIT, r#"{ type = "float", default = 100.0, min = 0.01, description = "Font pixels to one world unit, sizing the block the way a sprite is sized" }"#.into()),
        (k::LINE_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Baseline to baseline as a multiple of the size; zero takes the default" }"#.into()),
        (k::LETTER_SPACING, r#"{ type = "float", default = 0.0, description = "Extra space between glyphs, in font pixels" }"#.into()),
        (k::FONT_FAMILY, r#"{ type = "enum", default = "ui", options = ["ui", "heading", "mono", "icon"], description = "Which of the project's font chains to shape with" }"#.into()),
        (k::BITMAP_FONT, r#"{ type = "string", default = "", description = "A project-relative AngelCode .fnt naming a bitmap face; empty shapes with the project's vector fonts" }"#.into()),
        (k::OUTLINE_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "Font pixels the outline reaches around the glyphs; zero draws none" }"#.into()),
        (k::OUTLINE_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "The outline's colour" }"#.into()),
        (k::SHADOW_OFFSET_X, r#"{ type = "float", default = 0.0, description = "Font pixels the shadow is moved along x; zero with y draws none" }"#.into()),
        (k::SHADOW_OFFSET_Y, r#"{ type = "float", default = 0.0, description = "Font pixels the shadow is moved along y" }"#.into()),
        (k::SHADOW_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.5], description = "The shadow's colour" }"#.into()),
        (k::ALPHA_CUTOFF, r#"{ type = "float", default = 0.0, min = 0.0, max = 1.0, description = "Drop a pixel fainter than this and draw the rest opaque, as a material's `[surface] alpha_cutoff` does; zero blends every pixel" }"#.into()),
        (k::MATERIAL, format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The material every layer draws with, reading the glyph atlas as its texture; empty takes an inherited `material` component, else the built-in one. On `text2d` a material replaces `alpha_cutoff`" }}"#, crate::material::MATERIAL_ASSET_TYPE)),
    ]
    .into_iter()
    .chain(crate::world_text::Shaping::schema())
    .collect()
}

/// One colour key's channels, defaulting per channel: a decoration names two
/// colours, and neither is the node's `color`.
fn color_at(params: &toml::Value, key: &str, fallback: [f32; 4]) -> [f32; 4] {
    let channel = |i: usize| {
        params
            .get(key)
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(i))
            .and_then(balaur_core::components::as_f64)
            .map_or(fallback[i], |v| v as f32)
    };
    [channel(0), channel(1), channel(2), channel(3)]
}

/// One component's parameters read back into a renderable.
fn from_params(params: &toml::Value, in_3d: bool) -> Result<TextRenderable> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let number = |key: &str, fallback: f32| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .map_or(fallback, |v| v as f32)
    };
    let flag = |key: &str, fallback: bool| {
        params
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(fallback)
    };
    // -1, every layer, is how a scene spells an all-ones mask.
    let mask = |key: &str| {
        params
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .map_or(u32::MAX, |v| v as i64 as u32)
    };
    let max_width = number(k::MAX_WIDTH, 0.0);
    let (overlay_3d, overlay_2d) = if in_3d {
        (
            crate::overlay::overlay_3d(params),
            crate::overlay::Overlay2d::default(),
        )
    } else {
        (
            crate::overlay::Overlay3d::default(),
            crate::overlay::overlay_2d(params)?,
        )
    };
    Ok(TextRenderable {
        text: text(k::TEXT),
        text_key: text(k::TEXT_KEY),
        style: TextStyle {
            size: number(k::FONT_SIZE, 32.0).max(1.0),
            weight: number(k::FONT_WEIGHT, 400.0) as u16,
            color: crate::color_from_params(params),
            align: Align::of(&text(k::TEXT_ALIGN)),
            markup: flag(k::MARKUP, false),
            max_width: (max_width > 0.0).then_some(max_width),
            font: text(k::BITMAP_FONT),
            family: text(k::FONT_FAMILY),
            line_height: number(k::LINE_HEIGHT, 0.0).max(0.0),
            letter_spacing: number(k::LETTER_SPACING, 0.0),
            alpha_cutoff: number(k::ALPHA_CUTOFF, 0.0).clamp(0.0, 1.0),
            shaping: crate::world_text::Shaping::from_params(params),
            decoration: crate::world_text::Decoration {
                outline_size: number(k::OUTLINE_SIZE, 0.0).max(0.0),
                outline_color: color_at(params, k::OUTLINE_COLOR, [0.0, 0.0, 0.0, 1.0]),
                shadow_offset: [
                    number(k::SHADOW_OFFSET_X, 0.0),
                    number(k::SHADOW_OFFSET_Y, 0.0),
                ],
                shadow_color: color_at(params, k::SHADOW_COLOR, [0.0, 0.0, 0.0, 0.5]),
            },
        },
        pixels_per_unit: number(k::PIXELS_PER_UNIT, 100.0).max(0.01),
        in_3d,
        in_space: crate::world_text::SpaceOptions {
            billboard: flag(k::BILLBOARD, true),
            double_sided: flag(k::DOUBLE_SIDED, true),
            depth_test: flag(k::DEPTH_TEST, true),
            layers: crate::world_text::Layers3d {
                cast_shadow: flag(k::CAST_SHADOW, true),
                light_layers: mask(k::LIGHT_LAYERS),
                render_layers: mask(k::RENDER_LAYERS),
            },
        },
        material: text(k::MATERIAL),
        overlay_3d,
        overlay_2d,
        version: 0,
    })
}

/// A renderable back into the table a scene file would have written.
fn to_params(text: &TextRenderable) -> toml::Value {
    let mut out = toml::map::Map::new();
    let mut put = |key: &str, value: toml::Value| {
        out.insert(key.to_string(), value);
    };
    put(k::TEXT, toml::Value::String(text.text.clone()));
    put(k::TEXT_KEY, toml::Value::String(text.text_key.clone()));
    put(k::FONT_SIZE, toml::Value::Float(f64::from(text.style.size)));
    put(
        k::FONT_WEIGHT,
        toml::Value::Integer(i64::from(text.style.weight)),
    );
    put(k::COLOR, crate::color_to_toml(text.style.color));
    put(
        k::TEXT_ALIGN,
        toml::Value::String(text.style.align.word().into()),
    );
    put(
        k::MAX_WIDTH,
        toml::Value::Float(f64::from(text.style.max_width.unwrap_or(0.0))),
    );
    put(k::MARKUP, toml::Value::Boolean(text.style.markup));
    put(k::BITMAP_FONT, toml::Value::String(text.style.font.clone()));
    put(
        k::FONT_FAMILY,
        toml::Value::String(text.style.family.clone()),
    );
    put(
        k::LINE_HEIGHT,
        toml::Value::Float(f64::from(text.style.line_height)),
    );
    put(
        k::LETTER_SPACING,
        toml::Value::Float(f64::from(text.style.letter_spacing)),
    );
    let decoration = text.style.decoration;
    put(
        k::OUTLINE_SIZE,
        toml::Value::Float(f64::from(decoration.outline_size)),
    );
    put(
        k::OUTLINE_COLOR,
        crate::color_to_toml(decoration.outline_color),
    );
    put(
        k::SHADOW_OFFSET_X,
        toml::Value::Float(f64::from(decoration.shadow_offset[0])),
    );
    put(
        k::SHADOW_OFFSET_Y,
        toml::Value::Float(f64::from(decoration.shadow_offset[1])),
    );
    put(
        k::SHADOW_COLOR,
        crate::color_to_toml(decoration.shadow_color),
    );
    put(
        k::ALPHA_CUTOFF,
        toml::Value::Float(f64::from(text.style.alpha_cutoff)),
    );
    put(
        k::PIXELS_PER_UNIT,
        toml::Value::Float(f64::from(text.pixels_per_unit)),
    );
    put(k::MATERIAL, toml::Value::String(text.material.clone()));
    if text.in_3d {
        put(k::BILLBOARD, toml::Value::Boolean(text.in_space.billboard));
        put(
            k::DOUBLE_SIDED,
            toml::Value::Boolean(text.in_space.double_sided),
        );

        let layers = text.in_space.layers;
        put(k::CAST_SHADOW, toml::Value::Boolean(layers.cast_shadow));
        let mask = |bits: u32| toml::Value::Integer(i64::from(bits.cast_signed()));
        put(k::LIGHT_LAYERS, mask(layers.light_layers));
        put(k::RENDER_LAYERS, mask(layers.render_layers));
        crate::overlay::overlay_3d_to_map(&text.overlay_3d, &mut out);
    } else {
        crate::overlay::overlay_2d_to_map(&text.overlay_2d, &mut out);
    }
    text.style.shaping.put_into(&mut out);
    toml::Value::Table(out)
}

/// `text2d`: a block of text in the 2D pass, sized like a sprite.
pub(crate) fn register_text2d_component(reg: &mut Registry<'_>) {
    let mut schema = shared_schema();
    schema.extend(crate::overlay::schema_2d(crate::overlay::Drawn::Text2d));
    schema.sort_by(|a, b| a.0.cmp(b.0));
    let lines: Vec<(&str, &str)> = schema.iter().map(|(k, v)| (*k, v.as_str())).collect();
    reg.register_component(
        "text2d",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A block of `text` drawn in the 2D pass, `pixels_per_unit` font pixels per world unit.",
            schema: ComponentDef::parse_schema(
                "text2d",
                &balaur_core::components::ComponentDef::schema(&lines),
            ),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_text(eng, entity, from_params(params, false)?)
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<TextRenderable>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let text = world.get::<&TextRenderable>(entity).ok()?;
                (!text.in_3d).then(|| to_params(&text))
            }),
        },
    );
}

/// `text3d`: the same block in the 3D pass, facing the camera by default.
pub(crate) fn register_text3d_component(reg: &mut Registry<'_>) {
    let mut schema = shared_schema();
    schema.push((k::BILLBOARD, r#"{ type = "bool", default = true, description = "Turn to face the camera every frame; off leaves it in the node's own plane" }"#.into()));
    schema.push((k::DOUBLE_SIDED, r#"{ type = "bool", default = true, description = "Draw the back of the quad as well as the front" }"#.into()));
    schema.push((k::CAST_SHADOW, r#"{ type = "bool", default = true, description = "Whether it casts a shadow from the lights that cast. With `alpha_cutoff` above zero the shadow keeps the glyphs' shapes; at zero each glyph shadows as its whole quad" }"#.into()));
    schema.push((k::LIGHT_LAYERS, r#"{ type = "int", default = -1, description = "Light-layer bitmask; a `light3d` lights this when their masks share a bit. -1 is every layer" }"#.into()));
    schema.push((k::RENDER_LAYERS, r#"{ type = "int", default = -1, description = "Layer bitmask; a `camera3d` draws this when their `render_layers` share a bit. -1 is every layer" }"#.into()));
    schema.extend(crate::overlay::schema_3d(crate::overlay::Drawn::Text3d));
    schema.sort_by(|a, b| a.0.cmp(b.0));
    let lines: Vec<(&str, &str)> = schema.iter().map(|(k, v)| (*k, v.as_str())).collect();
    reg.register_component(
        "text3d",
        ComponentDef {
            events: &[],
            warnings: None,
            doc: "A block of `text` drawn in the 3D pass on a quad, `pixels_per_unit` font pixels per world unit; `billboard` turns it to the camera.",
            schema: ComponentDef::parse_schema(
                "text3d",
                &balaur_core::components::ComponentDef::schema(&lines),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_text(eng, entity, from_params(params, true)?)
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<TextRenderable>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let text = world.get::<&TextRenderable>(entity).ok()?;
                text.in_3d.then(|| to_params(&text))
            }),
        },
    );
}

/// `render.set_text` and `render.text`: the string a node draws, for a score
/// that changes every frame without rewriting the whole component.
pub(crate) fn install_text_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[]);
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
        match crate::world_text::shape_at(&app.engine, resolved, &text.style, *raster) {
            Ok(block) => blocks.push(Some(block)),
            Err(err) => {
                crate::world_text::report_once(&err);
                blocks.push(None);
            }
        }
        let _ = entity;
    }
    let texture = crate::world_text::atlas_texture(&app.engine);

    for ((entity, text, resolved, raster, from_parent), block) in wanted.into_iter().zip(blocks) {
        if let Some(mut old) = slots.remove(&entity) {
            old.detach();
        }
        let Some(block) = block else { continue };
        let Some(texture) = texture.clone() else {
            continue;
        };
        // Shaped at `raster`, wanted at `font_size`: the block is scaled back.
        let scale = (text.style.size / raster) / text.pixels_per_unit;
        let (custom_2d, custom_3d) =
            materials_of(app, &text, from_parent, (materials_2d, materials_3d));
        let mut slot = TextSlot {
            group_2d: None,
            two_d: Vec::new(),
            three_d: Vec::new(),
            colors: Vec::new(),
            version: text.version,
            raster,
            shaped: resolved,
            inherited: from_parent,
        };
        // Shadow, outline and text: a node each, drawn in that order.
        for (layer, (shifts, [r, g, b, a], picks)) in crate::world_text::layers(&block, &text.style)
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
                    node.enable_backface_culling(!text.in_space.double_sided);
                    node.set_color(tint);
                    let layers = text.in_space.layers;
                    node.set_casts_shadows(layers.cast_shadow)
                        .set_light_layers(layers.light_layers)
                        .set_render_layers(layers.render_layers);
                    crate::world_text::mask_3d(&mut node, text.style.alpha_cutoff);
                    node.set_depth_test(text.in_space.depth_test);
                    crate::overlay::apply_3d(&mut node, &text.overlay_3d);
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
                crate::world_text::mask_2d(&mut node, text.style.alpha_cutoff);
                crate::overlay::apply_2d(&mut node, &text.overlay_2d);
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
        });
        if rebuild {
            wanted.push((entity, text.clone(), resolved, raster, from_parent));
        }
    }
    (seen, wanted)
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
    let reference = if text.material.is_empty() {
        from_parent.reference().to_string()
    } else {
        text.material.clone()
    };
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
