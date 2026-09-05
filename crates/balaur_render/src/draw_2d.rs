//! Immediate 2D drawing: shapes a script asks for this frame and nothing
//! keeps. Filled shapes and pictures become nodes that live one frame;
//! arcs and polylines are the debug-line path with more segments. Like
//! debug lines, none of it is recorded: a replay re-runs the script that
//! drew, which draws again.

use anyhow::anyhow;
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

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
    /// Degrees, counter-clockwise from the x axis; width in pixels.
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
    /// A project image over a rect, tinted.
    Texture {
        path: String,
        center: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    },
}

/// What scripts drew this frame; the backend drains it as it draws.
#[derive(Default)]
pub struct DrawBuffer2d {
    pub shapes: Vec<Draw2d>,
}

fn color_of(args: &Value) -> anyhow::Result<[f32; 4]> {
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

fn points_of(list: &Value) -> anyhow::Result<Vec<[f32; 2]>> {
    let Value::List(items) = list else {
        return Err(anyhow!(
            "points should be a list of [x, y] pairs or vectors"
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Value::Vec2([x, y]) | Value::Vec3([x, y, _]) => Ok([*x, *y]),
            Value::List(pair) if pair.len() >= 2 => {
                let n = |v: &Value| match v {
                    Value::Num(n) => Ok(*n as f32),
                    Value::Int(n) => Ok(*n as f32),
                    other => Err(anyhow!("a coordinate should be a number, got {other:?}")),
                };
                Ok([n(&pair[0])?, n(&pair[1])?])
            }
            other => Err(anyhow!("a point is [x, y] or a vector, got {other:?}")),
        })
        .collect()
}

fn push(eng: &Engine, shape: Draw2d) {
    eng.resource::<DrawBuffer2d>()
        .borrow_mut()
        .shapes
        .push(shape);
}

pub(crate) fn install_draw_2d_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("draw_circle_2d", &[], "(x: float, y: float, radius: float, color: color)", "Fill a circle in world units for this frame, over everything the scene drew."),
        ("draw_rect_2d", &[], "(x: float, y: float, width: float, height: float, color: color)", "Fill a rectangle centred at a point, in world units, for this frame."),
        ("draw_arc_2d", &[], "(x: float, y: float, radius: float, from: float, to: float, width: float, color: color)", "Stroke an arc between two angles in degrees, counter-clockwise from the x axis, for this frame; width is in pixels."),
        ("draw_polyline_2d", &[], "(points: list, width: float, color: color)", "Stroke a chain of world-space points for this frame; width is in pixels."),
        ("draw_texture_2d", &[], "(path: string, x: float, y: float, width: float, height: float, color: color)", "Draw a project image over a rectangle centred at a point, in world units, for this frame; the colour tints it."),
    ]);
    m.function(
        "draw_circle_2d",
        |eng: &Engine, (x, y, radius, color): (f32, f32, f32, Option<Value>)| {
            push(
                eng,
                Draw2d::Circle {
                    center: [x, y],
                    radius: radius.max(0.0),
                    color: color_of(&color.unwrap_or(Value::Nil))?,
                },
            );
            Ok(())
        },
    );
    m.function(
        "draw_rect_2d",
        |eng: &Engine, (x, y, w, h, color): (f32, f32, f32, f32, Option<Value>)| {
            push(
                eng,
                Draw2d::Rect {
                    center: [x, y],
                    size: [w.max(0.0), h.max(0.0)],
                    color: color_of(&color.unwrap_or(Value::Nil))?,
                },
            );
            Ok(())
        },
    );
    m.function(
        "draw_arc_2d",
        |eng: &Engine,
         (x, y, radius, from, to, width, color): (
            f32,
            f32,
            f32,
            f32,
            f32,
            Option<f32>,
            Option<Value>,
        )| {
            push(
                eng,
                Draw2d::Arc {
                    center: [x, y],
                    radius: radius.max(0.0),
                    from,
                    to,
                    width: width.unwrap_or(1.0),
                    color: color_of(&color.unwrap_or(Value::Nil))?,
                },
            );
            Ok(())
        },
    );
    m.function(
        "draw_polyline_2d",
        |eng: &Engine, (points, width, color): (Value, Option<f32>, Option<Value>)| {
            push(
                eng,
                Draw2d::Polyline {
                    points: points_of(&points)?,
                    width: width.unwrap_or(1.0),
                    color: color_of(&color.unwrap_or(Value::Nil))?,
                },
            );
            Ok(())
        },
    );
    m.function(
        "draw_texture_2d",
        |eng: &Engine, (path, x, y, w, h, color): (String, f32, f32, f32, f32, Option<Value>)| {
            push(
                eng,
                Draw2d::Texture {
                    path,
                    center: [x, y],
                    size: [w.max(0.0), h.max(0.0)],
                    color: color_of(&color.unwrap_or(Value::Nil))?,
                },
            );
            Ok(())
        },
    );
}

/// Draw and forget: last frame's nodes go, this frame's are made.
#[cfg(feature = "kiss3d")]
pub(crate) fn flush(
    app: &balaur_core::App,
    window: &mut kiss3d::window::Window,
    scene: &mut kiss3d::scene::SceneNode2d,
    transients: &mut Vec<kiss3d::scene::SceneNode2d>,
) {
    use kiss3d::color::Color;

    for mut node in transients.drain(..) {
        node.detach();
    }
    let Some(buffer) = app.engine.try_resource::<DrawBuffer2d>() else {
        return;
    };
    let shapes = std::mem::take(&mut buffer.borrow_mut().shapes);
    for shape in shapes {
        match shape {
            Draw2d::Circle {
                center,
                radius,
                color: [r, g, b, a],
            } => {
                let mut node = scene.add_circle(radius);
                node.set_position(glamx::Vec2::new(center[0], center[1]))
                    .set_color(Color::new(r, g, b, a));
                transients.push(node);
            }
            Draw2d::Rect {
                center,
                size,
                color: [r, g, b, a],
            } => {
                let mut node = scene.add_rectangle(size[0], size[1]);
                node.set_position(glamx::Vec2::new(center[0], center[1]))
                    .set_color(Color::new(r, g, b, a));
                transients.push(node);
            }
            Draw2d::Texture {
                path,
                center,
                size,
                color: [r, g, b, a],
            } => {
                let mut node = scene.add_rectangle(size[0], size[1]);
                crate::texture::attach_texture_2d(&app.engine, &mut node, &path);
                node.set_position(glamx::Vec2::new(center[0], center[1]))
                    .set_color(Color::new(r, g, b, a));
                transients.push(node);
            }
            Draw2d::Arc {
                center,
                radius,
                from,
                to,
                width,
                color: [r, g, b, a],
            } => {
                let points = arc_points(center, radius, from, to);
                stroke(window, &points, width, Color::new(r, g, b, a));
            }
            Draw2d::Polyline {
                points,
                width,
                color: [r, g, b, a],
            } => stroke(window, &points, width, Color::new(r, g, b, a)),
        }
    }
}

#[cfg(feature = "kiss3d")]
fn stroke(
    window: &mut kiss3d::window::Window,
    points: &[[f32; 2]],
    width: f32,
    color: kiss3d::color::Color,
) {
    for pair in points.windows(2) {
        window.draw_line_2d(
            glamx::Vec2::new(pair[0][0], pair[0][1]),
            glamx::Vec2::new(pair[1][0], pair[1][1]),
            color,
            width,
        );
    }
}

/// An arc as a chain of points, one every few degrees; deterministic
/// trigonometry so a screenshot matches across machines.
#[cfg(any(feature = "kiss3d", test))]
pub(crate) fn arc_points(center: [f32; 2], radius: f32, from: f32, to: f32) -> Vec<[f32; 2]> {
    let sweep = to - from;
    let steps = ((sweep.abs() / 5.0).ceil() as usize).clamp(1, 360);
    (0..=steps)
        .map(|i| {
            let angle = (from + sweep * i as f32 / steps as f32).to_radians();
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
        let points = arc_points([0.0, 0.0], 2.0, 0.0, 90.0);
        assert!((points[0][0] - 2.0).abs() < 1e-5 && points[0][1].abs() < 1e-5);
        let last = points.last().unwrap();
        assert!(last[0].abs() < 1e-5 && (last[1] - 2.0).abs() < 1e-5);
        assert!(points.len() >= 4, "a quarter turn is more than one segment");
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
