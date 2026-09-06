//! The `text2d` and `text3d` components: a block of text a scene node carries.
//!
//! Both write a [`TextRenderable`]; the backend mirrors it as one mesh over the
//! shaper's atlas, rebuilt when the node's `version` moves or the atlas grows.
//! Separate from `world_text`'s immediate calls, which keep nothing.

use anyhow::{Result, anyhow};
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, entity_of};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, NodeId};

use crate::shape::keys as k;
use crate::shape::words;
use crate::world_text::{Align, TextRenderable, TextStyle};

/// The alignment words a scene and a script both spell.
pub(crate) const ALIGNMENTS: &[&str] = &["start", "center", "end"];

fn align_of(word: &str) -> Align {
    match word {
        "center" => Align::Center,
        "end" => Align::End,
        _ => Align::Start,
    }
}

fn align_word(align: Align) -> &'static str {
    match align {
        Align::Start => "start",
        Align::Center => "center",
        Align::End => "end",
    }
}

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
        (k::FONT_STYLE, r#"{ type = "enum", default = "normal", options = ["normal", "italic"], description = "Upright or italic" }"#.into()),
        (k::COLOR, r#"{ type = "color", default = [1.0, 1.0, 1.0, 1.0], description = "Tint, as channel floats or #rrggbb / #rrggbbaa" }"#.into()),
        (k::ALIGN, format!(r#"{{ type = "enum", default = "center", options = [{}], description = "Where the block sits across the node's origin" }}"#, crate::shape::options(ALIGNMENTS))),
        (k::MAX_WIDTH, r#"{ type = "float", default = 0.0, min = 0.0, description = "Font pixels the lines wrap at; zero runs the text on one line" }"#.into()),
        (k::MARKUP, r#"{ type = "bool", default = false, description = "Read the text as markup: bold, italic, colour, alignment, wave and inline images" }"#.into()),
        (k::PIXELS_PER_UNIT, r#"{ type = "float", default = 100.0, min = 0.01, description = "Font pixels to one world unit, sizing the block the way a sprite is sized" }"#.into()),
        (k::LINE_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Baseline to baseline as a multiple of the size; zero takes the default" }"#.into()),
        (k::LETTER_SPACING, r#"{ type = "float", default = 0.0, description = "Extra space between glyphs, in font pixels" }"#.into()),
        (k::FAMILY, r#"{ type = "enum", default = "ui", options = ["ui", "heading", "mono", "icons"], description = "Which of the project's font chains to shape with" }"#.into()),
        (k::FONT, r#"{ type = "string", default = "", description = "A project-relative AngelCode .fnt naming a bitmap face; empty shapes with the project's vector fonts" }"#.into()),
        (k::OUTLINE_SIZE, r#"{ type = "float", default = 0.0, min = 0.0, description = "Font pixels the outline reaches around the glyphs; zero draws none" }"#.into()),
        (k::OUTLINE_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 1.0], description = "The outline's colour" }"#.into()),
        (k::SHADOW_OFFSET_X, r#"{ type = "float", default = 0.0, description = "Font pixels the shadow is moved along x; zero with y draws none" }"#.into()),
        (k::SHADOW_OFFSET_Y, r#"{ type = "float", default = 0.0, description = "Font pixels the shadow is moved along y" }"#.into()),
        (k::SHADOW_COLOR, r#"{ type = "color", default = [0.0, 0.0, 0.0, 0.5], description = "The shadow's colour" }"#.into()),
    ]
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
fn from_params(params: &toml::Value, in_3d: bool) -> TextRenderable {
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
    let max_width = number(k::MAX_WIDTH, 0.0);
    TextRenderable {
        text: text(k::TEXT),
        text_key: text(k::TEXT_KEY),
        style: TextStyle {
            size: number(k::FONT_SIZE, 32.0).max(1.0),
            weight: number(k::FONT_WEIGHT, 400.0) as u16,
            italic: text(k::FONT_STYLE) == "italic",
            color: crate::color_from_params(params),
            align: align_of(&text(k::ALIGN)),
            markup: flag(k::MARKUP, false),
            max_width: (max_width > 0.0).then_some(max_width),
            font: text(k::FONT),
            family: text(k::FAMILY),
            line_height: number(k::LINE_HEIGHT, 0.0).max(0.0),
            letter_spacing: number(k::LETTER_SPACING, 0.0),
            alpha_cut: number(k::ALPHA_CUT, 0.0).clamp(0.0, 1.0),
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
        },
        version: 0,
    }
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
    put(
        k::FONT_STYLE,
        toml::Value::String(
            if text.style.italic {
                "italic"
            } else {
                "normal"
            }
            .into(),
        ),
    );
    put(k::COLOR, crate::color_to_toml(text.style.color));
    put(
        k::ALIGN,
        toml::Value::String(align_word(text.style.align).into()),
    );
    put(
        k::MAX_WIDTH,
        toml::Value::Float(f64::from(text.style.max_width.unwrap_or(0.0))),
    );
    put(k::MARKUP, toml::Value::Boolean(text.style.markup));
    put(k::FONT, toml::Value::String(text.style.font.clone()));
    put(k::FAMILY, toml::Value::String(text.style.family.clone()));
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
        k::PIXELS_PER_UNIT,
        toml::Value::Float(f64::from(text.pixels_per_unit)),
    );
    if text.in_3d {
        put(k::BILLBOARD, toml::Value::Boolean(text.in_space.billboard));
        put(
            k::DOUBLE_SIDED,
            toml::Value::Boolean(text.in_space.double_sided),
        );
        put(
            k::DEPTH_TEST,
            toml::Value::Boolean(text.in_space.depth_test),
        );
    }
    toml::Value::Table(out)
}

/// `text2d`: a block of text in the 2D pass, sized like a sprite.
pub(crate) fn register_text2d_component(reg: &mut Registry<'_>) {
    let mut schema = shared_schema();
    schema.sort_by(|a, b| a.0.cmp(b.0));
    let lines: Vec<(&str, &str)> = schema.iter().map(|(k, v)| (*k, v.as_str())).collect();
    reg.register_component(
        "text2d",
        ComponentDef {
            doc: "A block of text drawn in the 2D pass, shaped by the engine's fonts and sized at `pixels_per_unit` font pixels to the world unit.",
            schema: ComponentDef::parse_schema(
                "text2d",
                &balaur_core::components::ComponentDef::schema(&lines),
            ),
            tags: &[words::ORTHOGRAPHIC, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_text(eng, entity, from_params(params, false))
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
    schema.push((k::DEPTH_TEST, r#"{ type = "bool", default = true, description = "Let the scene hide it; off draws it over everything" }"#.into()));
    schema.sort_by(|a, b| a.0.cmp(b.0));
    let lines: Vec<(&str, &str)> = schema.iter().map(|(k, v)| (*k, v.as_str())).collect();
    reg.register_component(
        "text3d",
        ComponentDef {
            doc: "A block of text drawn in the 3D pass on a quad that faces the camera, shaped by the engine's fonts and sized at `pixels_per_unit` font pixels to the world unit.",
            schema: ComponentDef::parse_schema(
                "text3d",
                &balaur_core::components::ComponentDef::schema(&lines),
            ),
            tags: &[words::PERSPECTIVE, "render"],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                set_text(eng, entity, from_params(params, true))
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
    m.describe(&[
        ("set_text", &["text2d", "text3d"], "(node: node, text: string)", "Replace the text a node draws. The block re-shapes on the next frame; a `text_key` on the node still wins over it."),
        ("text", &["text2d", "text3d"], "(node: node)", "The text a node draws, as it was last set — not the localized string a `text_key` resolves to."),
    ]);
    m.function(
        "set_text",
        |eng: &Engine, (node, text): (NodeId, String)| {
            let entity = entity_of(node)?;
            let current = {
                let world = eng.world();
                world
                    .get::<&TextRenderable>(entity)
                    .ok()
                    .map(|t| (*t).clone())
            };
            let mut next = current.ok_or_else(|| anyhow!("node has no text2d or text3d"))?;
            next.text = text;
            set_text(eng, entity, next)?;
            Ok(())
        },
    );
    m.function("text", |eng: &Engine, node: NodeId| {
        let entity = entity_of(node)?;
        let world = eng.world();
        let text = world
            .get::<&TextRenderable>(entity)
            .map_err(|_| anyhow!("node has no text2d or text3d"))?;
        Ok(text.text.clone())
    });
}

/// One node's mesh and what it was built from.
#[cfg(feature = "kiss3d")]
pub(crate) struct TextSlot {
    /// One node per layer: shadow, outline, then the text itself.
    two_d: Vec<kiss3d::scene::SceneNode2d>,
    three_d: Vec<kiss3d::scene::SceneNode3d>,
    version: u64,
    /// The size the glyphs were rasterised at. A camera that moves changes
    /// how many pixels the block covers, and past a bucket it is re-shaped
    /// rather than magnified.
    raster: f32,
    /// The string last shaped, so a `text_key` that resolves differently
    /// after a language change rebuilds without the component moving.
    shaped: String,
}

#[cfg(feature = "kiss3d")]
impl TextSlot {
    /// Drop every node this slot made.
    fn detach(&mut self) {
        for mut node in self.two_d.drain(..) {
            node.detach();
        }
        for mut node in self.three_d.drain(..) {
            node.detach();
        }
    }
}

/// Mirror every `text2d` and `text3d` node as a mesh over the shaper's atlas.
///
/// Rebuilt when the component's version moves, when a `text_key` resolves to
/// something new, or when the atlas has grown under it and the old UVs point
/// at glyphs that are no longer there.
#[cfg(feature = "kiss3d")]
pub(crate) fn sync_text(
    app: &balaur_core::App,
    scene_2d: &mut kiss3d::scene::SceneNode2d,
    scene_3d: &mut kiss3d::scene::SceneNode3d,
    slots: &mut std::collections::HashMap<Entity, TextSlot>,
    viewport_height: f32,
) {
    use balaur_core::GlobalTransform;

    let world = app.engine.world();
    let mut seen: std::collections::HashSet<Entity> = std::collections::HashSet::new();
    let mut wanted: Vec<(Entity, TextRenderable, String, f32)> = Vec::new();
    for (entity, text, global) in &mut world.query::<(Entity, &TextRenderable, &GlobalTransform)>()
    {
        seen.insert(entity);
        let resolved = text.resolved(&app.engine);
        let raster = raster_size(app, &text, global, viewport_height);
        let rebuild = slots.get(&entity).is_none_or(|slot| {
            slot.version != text.version
                || slot.shaped != resolved
                || (slot.raster - raster).abs() > f32::EPSILON
        });
        if rebuild {
            wanted.push((entity, text.clone(), resolved, raster));
        }
    }
    drop(world);

    // Shape every block that moved before the atlas is uploaded, so one
    // upload covers the lot.
    let mut blocks = Vec::with_capacity(wanted.len());
    for (entity, text, resolved, raster) in &wanted {
        match crate::world_text::shape_at(&app.engine, resolved, &text.style, *raster) {
            Ok(block) => blocks.push(Some(block)),
            // The fonts install on the first UI pass, later in this frame:
            // said once, since the frame after it draws.
            Err(err) => {
                crate::world_text::warn_once(&err);
                blocks.push(None);
            }
        }
        let _ = entity;
    }
    let texture = crate::world_text::atlas_texture(&app.engine);
    if !wanted.is_empty() {
        tracing::info!(
            "TEXTSYNC wanted={} texture={} blocks={:?}",
            wanted.len(),
            texture.is_some(),
            blocks
                .iter()
                .map(|b| b.as_ref().map(|x| x.quads.len()))
                .collect::<Vec<_>>()
        );
    }

    for ((entity, text, resolved, raster), block) in wanted.into_iter().zip(blocks) {
        if let Some(mut old) = slots.remove(&entity) {
            old.detach();
        }
        let Some(block) = block else { continue };
        let Some(texture) = texture.clone() else {
            continue;
        };
        // Shaped at `raster`, wanted at `font_size`: the block is scaled back.
        let scale = (text.style.size / raster) / text.pixels_per_unit;
        let mut slot = TextSlot {
            two_d: Vec::new(),
            three_d: Vec::new(),
            version: text.version,
            raster,
            shaped: resolved,
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
                    slot.three_d.push(node);
                }
            } else if let Some(mesh) =
                crate::world_text::mesh_2d(&block, scale, text.style.align, &shifts, &picks)
            {
                let mut node = scene_2d.add_mesh(mesh, glamx::Vec2::ONE);
                node.set_texture(texture.clone());
                node.set_color(tint);
                slot.two_d.push(node);
            }
        }
        slots.insert(entity, slot);
    }

    place(app, slots, &seen);
}

/// Put every live block where its node is, and drop the ones whose node has
/// gone. Split from the rebuild above: one walks what changed, this walks all.
#[cfg(feature = "kiss3d")]
fn place(
    app: &balaur_core::App,
    slots: &mut std::collections::HashMap<Entity, TextSlot>,
    seen: &std::collections::HashSet<Entity>,
) {
    use balaur_core::{GlobalAppearance, GlobalTransform};

    // Where the eye is, for the blocks that face it.
    let eye = app
        .engine
        .try_resource::<crate::ViewportSnapshot>()
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
        let visible = world
            .get::<&GlobalAppearance>(*entity)
            .is_ok_and(|a| a.visible);
        let (angle, _, _) = global.rotation.to_euler(glamx::EulerRot::ZYX);
        for node in &mut slot.two_d {
            node.set_position(glamx::Vec2::new(global.position.x, global.position.y));
            node.set_rotation(angle);
            node.set_visible(visible);
        }
        // A billboard turns to the eye every frame; otherwise the block sits
        // in the node's own plane, like a sign painted on a wall.
        let turned = if text.in_space.billboard {
            eye.map_or(global.rotation, |eye| facing(eye - global.position))
        } else {
            global.rotation
        };
        for node in &mut slot.three_d {
            node.set_position(global.position);
            node.set_visible(visible);
            node.set_rotation(turned);
        }
        true
    });
}

/// The rotation that turns a quad's +z along `towards`, keeping its up as
/// close to the world's as it can — what a billboard needs.
#[cfg(feature = "kiss3d")]
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

/// The size a block's glyphs should be rasterised at: how many pixels one em
/// covers on screen, in buckets so a moving camera re-shapes rarely.
///
/// Falls back to the asked size when nothing has published a camera yet.
#[cfg(feature = "kiss3d")]
fn raster_size(
    app: &balaur_core::App,
    text: &TextRenderable,
    global: &balaur_core::GlobalTransform,
    viewport_height: f32,
) -> f32 {
    let em_world = text.style.size / text.pixels_per_unit.max(0.01);
    let per_unit = if text.in_3d {
        let Some(snapshot) = app.engine.try_resource::<crate::ViewportSnapshot>() else {
            return balaur_ui::text::bucket(text.style.size);
        };
        let snapshot = snapshot.borrow();
        let eye = glamx::Vec3::new(snapshot.eye[0], snapshot.eye[1], snapshot.eye[2]);
        let distance = (eye - global.position).length().max(0.01);
        // Half the frustum's height at that distance is what fills half the
        // viewport, so this is pixels to the world unit.
        let half = (snapshot.fov / 2.0).tan().max(1e-4) * distance;
        viewport_height / (2.0 * half)
    } else {
        let Some(snapshot) = app.engine.try_resource::<crate::ViewportSnapshot2d>() else {
            return balaur_ui::text::bucket(text.style.size);
        };
        // The 2D camera's zoom is already pixels to the world unit.
        snapshot.borrow().zoom.max(0.01)
    };
    balaur_ui::text::bucket((em_world * per_unit).clamp(1.0, 512.0))
}
