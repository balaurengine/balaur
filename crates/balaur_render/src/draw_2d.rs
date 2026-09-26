//! Immediate 2D drawing: shapes a script asks for this frame and nothing
//! keeps. Filled shapes and pictures become nodes that live one frame;
//! arcs and polylines are the debug-line path with more segments. Like
//! debug lines, none of it is recorded: a replay re-runs the script that
//! drew, which draws again.
//!
//! Every verb takes a trailing options table. `z_index` puts the shape in
//! the scene's draw order among the nodes of that index, over them and under
//! the next; without it the shape draws over everything.

use anyhow::{anyhow, bail};
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::keys as k;

/// One shape, in world units, with a colour as channel floats.
#[derive(Clone, Debug, PartialEq)]
pub enum Draw2d {
    Circle {
        center: [f32; 2],
        radius: f32,
        color: [f32; 4],
    },
    Rect {
        center: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    },
    /// Radians, counter-clockwise from the x axis; width in pixels.
    Arc {
        center: [f32; 2],
        radius: f32,
        from: f32,
        to: f32,
        width: f32,
        color: [f32; 4],
    },
    Polyline {
        points: Vec<[f32; 2]>,
        width: f32,
        color: [f32; 4],
    },
    /// A filled convex outline. A concave shape is triangulated by the
    /// caller through `geometry2d` and filled a triangle at a time.
    Polygon {
        points: Vec<[f32; 2]>,
        color: [f32; 4],
    },
    /// A project image over a rect, tinted; `region` is the part of it drawn,
    /// `[x, y, width, height]` in the image's pixels.
    Texture {
        path: String,
        center: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        region: Option<[f32; 4]>,
    },
}

/// A shape and its place in the draw order: a `z_index`, or over everything.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn2d {
    pub shape: Draw2d,
    pub z_index: Option<i32>,
}

/// What scripts drew this frame; the backend drains it as it draws.
#[derive(Default)]
pub struct DrawBuffer2d {
    pub shapes: Vec<Drawn2d>,
}

/// A verb's trailing options table.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Options {
    pub z_index: Option<i32>,
    pub region: Option<[f32; 4]>,
}

pub(crate) fn color_of(args: &Value) -> anyhow::Result<[f32; 4]> {
    match args {
        Value::Nil => Ok([1.0, 1.0, 1.0, 1.0]),
        Value::Color(c) => Ok(*c),
        Value::List(items) if items.len() >= 3 => {
            let channel = |i: usize| match items.get(i) {
                Some(Value::Num(n)) => Ok(*n as f32),
                Some(Value::Int(n)) => Ok(*n as f32),
                None => Ok(1.0),
                other => Err(anyhow!(
                    "a colour channel should be a number, got {other:?}"
                )),
            };
            Ok([channel(0)?, channel(1)?, channel(2)?, channel(3)?])
        }
        other => Err(anyhow!(
            "a colour is a color value or a list of channel floats, got {other:?}"
        )),
    }
}

fn number(value: &Value) -> anyhow::Result<f32> {
    match value {
        Value::Num(n) => Ok(*n as f32),
        Value::Int(n) => Ok(*n as f32),
        other => Err(anyhow!("a coordinate should be a number, got {other:?}")),
    }
}

fn point_of(item: &Value) -> anyhow::Result<[f32; 2]> {
    match item {
        Value::Vec2([x, y]) | Value::Vec3([x, y, _]) => Ok([*x, *y]),
        Value::List(pair) if pair.len() >= 2 => Ok([number(&pair[0])?, number(&pair[1])?]),
        other => Err(anyhow!("a point is [x, y] or a vector, got {other:?}")),
    }
}

fn points_of(list: &Value) -> anyhow::Result<Vec<[f32; 2]>> {
    let Value::List(items) = list else {
        return Err(anyhow!(
            "points should be a list of [x, y] pairs or vectors"
        ));
    };
    items.iter().map(point_of).collect()
}

/// The options a verb was handed; `region_*` is read only where `texture`
/// says a picture can take one.
pub(crate) fn options_of(opts: Option<Value>, texture: bool) -> anyhow::Result<Options> {
    let entries = match opts {
        None | Some(Value::Nil) => return Ok(Options::default()),
        Some(Value::Map(entries)) => entries,
        Some(other) => bail!("draw options are a table, got {other:?}"),
    };
    let mut out = Options::default();
    let (mut origin, mut size) = (None, None);
    for (key, value) in &entries {
        match key.as_str() {
            k::Z_INDEX => out.z_index = Some(number(value)? as i32),
            k::REGION_ORIGIN if texture => origin = Some(point_of(value)?),
            k::REGION_SIZE if texture => size = Some(point_of(value)?),
            other if texture => bail!(
                "draw options take {}, {} and {}, not '{other}'",
                k::Z_INDEX,
                k::REGION_ORIGIN,
                k::REGION_SIZE
            ),
            other => bail!("draw options take {}, not '{other}'", k::Z_INDEX),
        }
    }
    out.region = match (origin, size) {
        (_, Some([w, h])) => {
            let [x, y] = origin.unwrap_or([0.0, 0.0]);
            Some([x, y, w, h])
        }
        (Some(_), None) => bail!(
            "a {} needs the {} it starts",
            k::REGION_ORIGIN,
            k::REGION_SIZE
        ),
        (None, None) => None,
    };
    Ok(out)
}

fn push(eng: &Engine, shape: Draw2d, opts: Option<Value>) -> anyhow::Result<()> {
    let options = options_of(opts, false)?;
    push_at(eng, shape, options.z_index);
    Ok(())
}

pub(crate) fn push_at(eng: &Engine, shape: Draw2d, z_index: Option<i32>) {
    eng.resource::<DrawBuffer2d>()
        .borrow_mut()
        .shapes
        .push(Drawn2d { shape, z_index });
}

/// `draw_arc_2d`'s arguments: centre, radius, the two angles, width, colour
/// and options.
type ArcArgs = (
    f32,
    f32,
    f32,
    f32,
    f32,
    Option<f32>,
    Option<Value>,
    Option<Value>,
);

/// `draw_texture_2d`'s arguments: the path, centre, size, tint and options.
type TextureArgs = (String, f32, f32, f32, f32, Option<Value>, Option<Value>);

pub(crate) fn install_draw_2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("draw_circle_2d", &[], "(x: float, y: float, radius: float, color: color, opts: table)", "Fill a circle in world units for this frame, over everything the scene drew; `opts.z_index` places it among the nodes of that index instead."),
        ("draw_rect_2d", &[], "(x: float, y: float, width: float, height: float, color: color, opts: table)", "Fill a rectangle centred at a point, in world units, for this frame; `opts` takes `z_index`."),
        ("draw_arc_2d", &[], "(x: float, y: float, radius: float, from: float, to: float, width: float, color: color, opts: table)", "Stroke an arc between two angles in radians, counter-clockwise from the x axis, for this frame; width is in pixels, and `opts` takes `z_index`."),
        ("draw_polygon_2d", &[], "(points: list, color: color, opts: table)", "Fill a convex outline of world-space points for this frame; `geometry2d` cuts a concave one into triangles first. `opts` takes `z_index`."),
        ("draw_polyline_2d", &[], "(points: list, width: float, color: color, opts: table)", "Stroke a chain of world-space points for this frame; width is in pixels, and `opts` takes `z_index`."),
        ("draw_texture_2d", &[], "(path: string, x: float, y: float, width: float, height: float, color: color, opts: table)", "Draw a project image over a rectangle centred at a point, in world units, for this frame; the colour tints it. `opts` takes `z_index`, and `region_origin` and `region_size` in the image's pixels for part of it."),
    ]);
    m.function(
        "draw_circle_2d",
        |eng: &Engine,
         (x, y, radius, color, opts): (f32, f32, f32, Option<Value>, Option<Value>)| {
            let shape = Draw2d::Circle {
                center: [x, y],
                radius: radius.max(0.0),
                color: color_of(&color.unwrap_or(Value::Nil))?,
            };
            push(eng, shape, opts)
        },
    );
    m.function(
        "draw_rect_2d",
        |eng: &Engine,
         (x, y, w, h, color, opts): (f32, f32, f32, f32, Option<Value>, Option<Value>)| {
            let shape = Draw2d::Rect {
                center: [x, y],
                size: [w.max(0.0), h.max(0.0)],
                color: color_of(&color.unwrap_or(Value::Nil))?,
            };
            push(eng, shape, opts)
        },
    );
    m.function(
        "draw_arc_2d",
        |eng: &Engine, (x, y, radius, from, to, width, color, opts): ArcArgs| {
            let shape = Draw2d::Arc {
                center: [x, y],
                radius: radius.max(0.0),
                from,
                to,
                width: width.unwrap_or(crate::DEFAULT_LINE_WIDTH),
                color: color_of(&color.unwrap_or(Value::Nil))?,
            };
            push(eng, shape, opts)
        },
    );
    install_outline_api(m);
    m.function(
        "draw_texture_2d",
        |eng: &Engine, (path, x, y, w, h, color, opts): TextureArgs| {
            let options = options_of(opts, true)?;
            let shape = Draw2d::Texture {
                path,
                center: [x, y],
                size: [w.max(0.0), h.max(0.0)],
                color: color_of(&color.unwrap_or(Value::Nil))?,
                region: options.region,
            };
            push_at(eng, shape, options.z_index);
            Ok(())
        },
    );
}

/// The outline half of the 2D immediate API, split from
/// [`install_draw_2d_api`] under `MAX_FN_LINES`: a filled polygon and a
/// stroked chain of points.
fn install_outline_api(m: &mut dyn Bindings<Engine>) {
    m.function(
        "draw_polygon_2d",
        |eng: &Engine, (points, color, opts): (Value, Option<Value>, Option<Value>)| {
            let shape = Draw2d::Polygon {
                points: points_of(&points)?,
                color: color_of(&color.unwrap_or(Value::Nil))?,
            };
            push(eng, shape, opts)
        },
    );
    m.function(
        "draw_polyline_2d",
        |eng: &Engine,
         (points, width, color, opts): (Value, Option<f32>, Option<Value>, Option<Value>)| {
            let shape = Draw2d::Polyline {
                points: points_of(&points)?,
                width: width.unwrap_or(crate::DEFAULT_LINE_WIDTH),
                color: color_of(&color.unwrap_or(Value::Nil))?,
            };
            push(eng, shape, opts)
        },
    );
}

/// One empty node per `z_index` scripts draw at, which `sync_2d` places in
/// the scene's draw order; what is drawn at that index hangs under it. A
/// holder stays once made: kiss3d's `detach` swaps the last child into the
/// gap, so taking one out of the middle would scramble the order.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct Layers2d {
    holders: std::collections::BTreeMap<i32, kiss3d::scene::SceneNode2d>,
}

#[cfg(feature = "window")]
impl Layers2d {
    /// A holder for every index this frame's shapes and text name.
    pub(crate) fn want(&mut self, app: &balaur_core::App) {
        let mut wanted = Vec::new();
        if let Some(buffer) = app.engine.try_resource::<DrawBuffer2d>() {
            wanted.extend(buffer.borrow().shapes.iter().filter_map(|d| d.z_index));
        }
        if let Some(buffer) = app
            .engine
            .try_resource::<crate::world_text::TextDrawBuffer>()
        {
            let items = &buffer.borrow().items;
            wanted.extend(items.iter().filter(|t| !t.in_3d).filter_map(|t| t.z_index));
        }
        for z in wanted {
            self.holders
                .entry(z)
                .or_insert_with(kiss3d::scene::SceneNode2d::empty);
        }
    }

    /// The indices that have a holder, lowest first.
    pub(crate) fn indices(&self) -> impl Iterator<Item = i32> + '_ {
        self.holders.keys().copied()
    }

    pub(crate) fn holder(&self, z_index: i32) -> Option<kiss3d::scene::SceneNode2d> {
        self.holders.get(&z_index).cloned()
    }

    /// Where a drawing at `z_index` goes: its holder, or over everything.
    pub(crate) fn parent_of(
        &self,
        z_index: Option<i32>,
        scene: &kiss3d::scene::SceneNode2d,
    ) -> (kiss3d::scene::SceneNode2d, bool) {
        match z_index.and_then(|z| self.holder(z)) {
            Some(holder) => (holder, true),
            None => (scene.clone(), false),
        }
    }
}

/// Draw and forget: last frame's nodes go, this frame's are made.
#[cfg(feature = "window")]
pub(crate) fn flush(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    scene: &kiss3d::scene::SceneNode2d,
    layers: &Layers2d,
    transients: &mut Vec<kiss3d::scene::SceneNode2d>,
) {
    for mut node in transients.drain(..) {
        node.detach();
    }
    let Some(buffer) = app.engine.try_resource::<DrawBuffer2d>() else {
        return;
    };
    let shapes = std::mem::take(&mut buffer.borrow_mut().shapes);
    let per_pixel = app
        .engine
        .try_resource::<crate::ViewportSnapshot2d>()
        .map(|vp| vp.borrow().zoom)
        .filter(|zoom| *zoom > 0.0)
        .map_or(1.0, |zoom| 1.0 / zoom);
    for Drawn2d { shape, z_index } in shapes {
        let (mut parent, placed) = layers.parent_of(z_index, scene);
        let placed = placed.then_some(per_pixel);
        if let Some(node) = draw_one(app, window, &mut parent, shape, placed) {
            transients.push(node);
        }
    }
}

/// One shape as a node under `parent`, or as the window's lines. `placed`
/// is the world units to a pixel when the shape sits in the draw order.
#[cfg(feature = "window")]
fn draw_one(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    parent: &mut kiss3d::scene::SceneNode2d,
    shape: Draw2d,
    placed: Option<f32>,
) -> Option<kiss3d::scene::SceneNode2d> {
    use glamx::Vec2;
    use kiss3d::color::Color;

    let paint = |[r, g, b, a]: [f32; 4]| Color::new(r, g, b, a);
    match shape {
        Draw2d::Circle {
            center,
            radius,
            color,
        } => {
            let mut node = parent.add_circle(radius);
            node.set_position(Vec2::new(center[0], center[1]))
                .set_color(paint(color));
            Some(node)
        }
        Draw2d::Rect {
            center,
            size,
            color,
        } => {
            let mut node = parent.add_rectangle(size[0], size[1]);
            node.set_position(Vec2::new(center[0], center[1]))
                .set_color(paint(color));
            Some(node)
        }
        Draw2d::Texture {
            path,
            center,
            size,
            color,
            region,
        } => {
            let mut node = parent.add_rectangle(size[0], size[1]);
            crate::texture::attach_texture_2d(&app.engine, &mut node, &path);
            // In the pixels the image was drawn at, as a sprite's region is.
            if let Some([x, y, w, h]) = region
                && let Ok((tw, th)) = crate::texture::size_of(&app.engine, &path)
                && tw > 0
                && th > 0
            {
                let (tw, th) = (tw as f32, th as f32);
                node.set_uv_rect(
                    Vec2::new(x / tw, y / th),
                    Vec2::new((x + w) / tw, (y + h) / th),
                );
            }
            node.set_position(Vec2::new(center[0], center[1]))
                .set_color(paint(color));
            Some(node)
        }
        Draw2d::Arc {
            center,
            radius,
            from,
            to,
            width,
            color,
        } => {
            let points = arc_points(center, radius, from, to);
            stroke(window, parent, &points, width, paint(color), placed)
        }
        Draw2d::Polyline {
            points,
            width,
            color,
        } => stroke(window, parent, &points, width, paint(color), placed),
        Draw2d::Polygon { points, color } => (points.len() >= 3).then(|| {
            let outline = points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
            let mut node = parent.add_convex_polygon(outline, Vec2::ONE);
            node.set_color(paint(color));
            node
        }),
    }
}

/// A stroke over everything is the window's line pass, in pixels; one placed
/// among the nodes is a mesh, `placed` world units to the pixel.
#[cfg(feature = "window")]
fn stroke(
    window: &mut kiss3d::window::Window,
    parent: &mut kiss3d::scene::SceneNode2d,
    points: &[[f32; 2]],
    width: f32,
    color: kiss3d::color::Color,
    placed: Option<f32>,
) -> Option<kiss3d::scene::SceneNode2d> {
    use glamx::Vec2;

    let Some(per_pixel) = placed else {
        for pair in points.windows(2) {
            window.draw_line_2d(
                Vec2::new(pair[0][0], pair[0][1]),
                Vec2::new(pair[1][0], pair[1][1]),
                color,
                width,
            );
        }
        return None;
    };
    let chain: Vec<Vec2> = points.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    let style = balaur_core::stroke::Stroke {
        width: width * per_pixel,
        ..balaur_core::stroke::Stroke::default()
    };
    let pieces = balaur_core::stroke::stroke(&chain, &style, 1);
    if pieces.is_empty() {
        return None;
    }
    let mut group = parent.add_group();
    for piece in pieces {
        let mesh =
            kiss3d::resource::GpuMesh2d::new(piece.coords, piece.faces, Some(piece.uvs), false);
        let mut node = group.add_mesh(std::rc::Rc::new(std::cell::RefCell::new(mesh)), Vec2::ONE);
        node.set_color(color);
    }
    Some(group)
}

/// An arc as a chain of points, one every few degrees; deterministic
/// trigonometry so a screenshot matches across machines.
#[cfg(any(feature = "window", test))]
pub(crate) fn arc_points(center: [f32; 2], radius: f32, from: f32, to: f32) -> Vec<[f32; 2]> {
    let sweep = to - from;
    let steps = ((sweep.abs().to_degrees() / 5.0).ceil() as usize).clamp(1, 360);
    (0..=steps)
        .map(|i| {
            let angle = from + sweep * i as f32 / steps as f32;
            let (sin, cos) = libm::sincosf(angle);
            [center[0] + cos * radius, center[1] + sin * radius]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arc_starts_and_ends_on_its_angles() {
        let points = arc_points([0.0, 0.0], 2.0, 0.0, core::f32::consts::FRAC_PI_2);
        assert!((points[0][0] - 2.0).abs() < 1e-5 && points[0][1].abs() < 1e-5);
        let last = points.last().unwrap();
        assert!(last[0].abs() < 1e-5 && (last[1] - 2.0).abs() < 1e-5);
        assert!(points.len() >= 4, "a quarter turn is more than one segment");
    }

    fn table(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn a_verb_s_options_place_it_and_cut_a_region_from_a_picture() {
        let placed = options_of(Some(table(&[(k::Z_INDEX, Value::Int(3))])), false).unwrap();
        assert_eq!(placed.z_index, Some(3));
        assert_eq!(options_of(None, false).unwrap(), Options::default());
        let cut = Some(table(&[
            (k::REGION_ORIGIN, Value::Vec2([16.0, 8.0])),
            (k::REGION_SIZE, Value::Vec2([32.0, 24.0])),
        ]));
        assert_eq!(
            options_of(cut.clone(), true).unwrap().region,
            Some([16.0, 8.0, 32.0, 24.0])
        );
        assert!(
            options_of(cut, false).is_err(),
            "a circle has no picture to cut"
        );
        let size_alone = Some(table(&[(k::REGION_SIZE, Value::Vec2([4.0, 4.0]))]));
        assert_eq!(
            options_of(size_alone, true).unwrap().region,
            Some([0.0, 0.0, 4.0, 4.0])
        );
        let typo = Some(table(&[("z", Value::Int(1))]));
        assert!(
            options_of(typo, false).is_err(),
            "a misspelt key is refused"
        );
    }

    #[test]
    fn a_colour_may_be_a_list_of_three_or_four_channels() {
        let close = |got: [f32; 4], want: [f32; 4]| {
            got.iter()
                .zip(want.iter())
                .all(|(a, b)| (a - b).abs() < 1e-6)
        };
        let rgb = Value::List(vec![Value::Num(0.5), Value::Int(1), Value::Num(0.0)]);
        assert!(close(color_of(&rgb).unwrap(), [0.5, 1.0, 0.0, 1.0]));
        assert!(close(color_of(&Value::Nil).unwrap(), [1.0; 4]));
        assert!(color_of(&Value::Str("red".into())).is_err());
    }
}
