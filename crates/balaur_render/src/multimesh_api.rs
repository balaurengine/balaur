//! The script handle a `multimesh3d` or `multimesh2d` node is driven
//! through: one instance at a time, the counts, and every instance at once.
//! Split from `multimesh` for its length.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::as_f64;
use balaur_core::entity_of;
use balaur_script::{Module, NodeId, Value};
use glamx::{Affine2, Affine3A, Mat4};

use crate::multimesh::{
    Instance, MAX_INSTANCES, MULTIMESH_2D, MULTIMESH_3D, MultiMesh, buffer_of,
    instances_from_buffer,
};
use crate::vocabulary::keys as k;

const BUFFER: &str = "buffer";
const INSTANCE_COLOR: &str = "instance_color";
const INSTANCE_COUNT: &str = "instance_count";
const INSTANCE_CUSTOM_DATA: &str = "instance_custom_data";
const INSTANCE_REGION: &str = "instance_region";
const INSTANCE_TRANSFORM: &str = "instance_transform";
const INSTANCES: &str = "instances";
const SET_BUFFER: &str = "set_buffer";
const SET_INSTANCE_COLOR: &str = "set_instance_color";
const SET_INSTANCE_COUNT: &str = "set_instance_count";
const SET_INSTANCE_CUSTOM_DATA: &str = "set_instance_custom_data";
const SET_INSTANCE_REGION: &str = "set_instance_region";
const SET_INSTANCE_TRANSFORM: &str = "set_instance_transform";
const SET_INSTANCES: &str = "set_instances";
const SET_VISIBLE_INSTANCE_COUNT: &str = "set_visible_instance_count";

/// The node's multimesh, for a handle method to read or write.
fn with_multimesh<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut MultiMesh) -> Result<R>,
) -> Result<R> {
    let entity = entity_of(node)?;
    let world = eng.world();
    let mut multimesh = world
        .get::<&mut MultiMesh>(entity)
        .map_err(|_| anyhow!("the node carries no {MULTIMESH_3D} or {MULTIMESH_2D}"))?;
    f(&mut multimesh)
}

/// An index into the node's instances, or an error naming how many it holds.
fn slot(multimesh: &MultiMesh, index: i64) -> Result<usize> {
    usize::try_from(index)
        .ok()
        .filter(|i| *i < multimesh.instances.len())
        .ok_or_else(|| {
            anyhow!(
                "instance {index} is past the end: the multimesh holds {}",
                multimesh.instances.len()
            )
        })
}

/// A count a script asks for, held to what a node may carry.
fn count_of(count: i64) -> Result<usize> {
    usize::try_from(count)
        .ok()
        .filter(|n| *n <= MAX_INSTANCES)
        .ok_or_else(|| anyhow!("a multimesh holds 0 to {MAX_INSTANCES} instances, not {count}"))
}

/// Set an instance's transform from a `Transform3d`, or from a `Transform2d`
/// in the xy plane. A shear either holds is kept, as a basis.
fn place(instance: &mut Instance, transform: &Value) -> Result<()> {
    match transform {
        Value::Transform3d(columns) => {
            instance.set_local(Mat4::from(Affine3A::from_cols_array(columns)));
        }
        Value::Transform2d(columns) => {
            let flat = Affine2::from_cols_array(columns);
            instance.set_local(Mat4::from_cols(
                flat.matrix2.x_axis.extend(0.0).extend(0.0),
                flat.matrix2.y_axis.extend(0.0).extend(0.0),
                glamx::Vec4::Z,
                flat.translation.extend(0.0).extend(1.0),
            ));
        }
        other => bail!(
            "an instance's transform is a Transform3d or a Transform2d, not a {}",
            other.type_name()
        ),
    }
    Ok(())
}

/// An instance's transform as its node's dimension spells one.
fn transform_of(instance: &Instance, flat: bool) -> Value {
    let m = instance.local();
    if flat {
        Value::Transform2d(
            Affine2::from_cols(
                m.x_axis.truncate().truncate(),
                m.y_axis.truncate().truncate(),
                m.w_axis.truncate().truncate(),
            )
            .to_cols_array(),
        )
    } else {
        Value::Transform3d(Affine3A::from_mat4(m).to_cols_array())
    }
}

/// Two floats from a `Vec2` or a list of two numbers.
fn two(value: &Value, what: &str) -> Result<[f32; 2]> {
    match value {
        Value::Vec2(v) => Ok(*v),
        Value::List(items) => match items.as_slice() {
            [x, y] => Ok([number_of(x, what)?, number_of(y, what)?]),
            _ => bail!("{what} is two numbers, not {}", items.len()),
        },
        other => bail!(
            "{what} is a Vec2 or two numbers, not a {}",
            other.type_name()
        ),
    }
}

fn number_of(value: &Value, what: &str) -> Result<f32> {
    match value {
        Value::Num(n) => Ok(*n as f32),
        Value::Int(n) => Ok(*n as f32),
        other => bail!("{what} holds a {}, not a number", other.type_name()),
    }
}

fn four(value: &Value, what: &str) -> Result<[f32; 4]> {
    match value {
        Value::Color(c) => Ok(*c),
        other => bail!("{what} is a Color, not a {}", other.type_name()),
    }
}

/// An instance as `instances()` hands it over: the asset's own keys, each a
/// plain list of numbers as the file spells it, so a list read here saves
/// straight into a `multimesh` file or a scene's `[[assets]]` block.
pub(crate) fn instance_value(instance: &Instance) -> Value {
    let toml::Value::Table(table) = instance.to_table() else {
        return Value::Nil;
    };
    Value::Map(
        table
            .into_iter()
            .map(|(key, value)| {
                let value = match value {
                    toml::Value::Array(items) => {
                        Value::List(items.iter().filter_map(as_f64).map(Value::Num).collect())
                    }
                    other => as_f64(&other).map_or(Value::Nil, Value::Num),
                };
                (key, value)
            })
            .collect(),
    )
}

/// One of the two handles. Each is a module of its own, as each answers its
/// own transform.
pub(crate) struct Handle {
    pub(crate) component: &'static str,
    pub(crate) on: &'static [&'static str],
    pub(crate) doc: &'static str,
    set_transform: &'static str,
    transform: &'static str,
}

pub(crate) const HANDLES: [Handle; 2] = [
    Handle {
        component: MULTIMESH_3D,
        on: &[MULTIMESH_3D],
        doc: "Thousands of copies of one mesh in one draw, each with its own transform, colour and custom data, through `node.multimesh3d`.",
        set_transform: "(node: node, index: int, transform: Transform3d)",
        transform: "(node: node, index: int) -> Transform3d",
    },
    Handle {
        component: MULTIMESH_2D,
        on: &[MULTIMESH_2D],
        doc: "Thousands of copies of one sprite in one draw, each with its own transform, colour and custom data, through `node.multimesh2d`.",
        set_transform: "(node: node, index: int, transform: Transform2d)",
        transform: "(node: node, index: int) -> Transform2d",
    },
];

/// What `instances()` hands over and `set_instances` takes: the `multimesh`
/// asset's own keys.
pub(crate) fn install_multimesh_records(m: &mut Module<Engine>) {
    m.records(&[(
        "MultimeshInstance",
        "{ position: Vec3?, rotation_euler: Vec3?, scale: Vec3?, basis: [float]?, color: Color?, custom: Color?, region_origin: Vec2?, region_size: Vec2? }",
        "One instance as a `multimesh` asset spells it. `basis` stands in for `rotation_euler` and `scale` when the instance is sheared.",
    ), (
        "InstanceRegion",
        "{ origin: Vec2, size: Vec2 }",
        "The rectangle of a texture one instance draws, in pixels.",
    )]);
}

/// One instance at a time on a multimesh handle: Godot's `MultiMesh` calls,
/// readers without their `get_`.
pub(crate) fn install_multimesh_api(m: &mut Module<Engine>, handle: &Handle) {
    let on = handle.on;
    m.describe(&[
        (SET_INSTANCE_TRANSFORM, on, handle.set_transform, "Place one instance. A shear is kept, as the instance's `basis`."),
        (SET_INSTANCE_REGION, &[MULTIMESH_2D], "(node: node, index: int, origin: Vec2, size: Vec2)", "The rectangle of the texture one instance draws, as an origin and a size in texture pixels; a zero size draws all of it. Not drawn yet: `multimesh2d` draws through Balaur's own pipeline, which binds no rectangle per instance."),
        (INSTANCE_REGION, &[MULTIMESH_2D], "(node: node, index: int) -> render::InstanceRegion?", "One instance's texture rectangle in pixels, or nil when it draws the whole texture."),
        (INSTANCE_TRANSFORM, on, handle.transform, "One instance's transform."),
        (SET_INSTANCE_COLOR, on, "(node: node, index: int, color: Color)", "The colour one instance draws in."),
        (INSTANCE_COLOR, on, "(node: node, index: int) -> Color", "The colour one instance draws in."),
        (SET_INSTANCE_CUSTOM_DATA, on, "(node: node, index: int, data: Color)", "Four floats a material's shader reads for one instance, as a colour."),
        (INSTANCE_CUSTOM_DATA, on, "(node: node, index: int) -> Color", "One instance's four floats of custom data, as a colour."),
    ]);
    m.function(
        SET_INSTANCE_TRANSFORM,
        |eng: &Engine, (node, index, transform): (NodeId, i64, Value)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                place(&mut multimesh.instances[at], &transform)
            })
        },
    );
    m.function(
        SET_INSTANCE_REGION,
        |eng: &Engine, (node, index, origin, size): (NodeId, i64, Value, Value)| {
            let (origin, size) = (two(&origin, k::REGION_ORIGIN)?, two(&size, k::REGION_SIZE)?);
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                multimesh.instances[at].region = (size[0] > 0.0 && size[1] > 0.0)
                    .then_some([origin[0], origin[1], size[0], size[1]]);
                Ok(())
            })
        },
    );
    m.function(
        INSTANCE_REGION,
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(multimesh.instances[at]
                    .region
                    .map_or(Value::Nil, |[x, y, w, h]| {
                        Value::Map(vec![
                            (k::ORIGIN.into(), Value::Vec2([x, y])),
                            (k::SIZE.into(), Value::Vec2([w, h])),
                        ])
                    }))
            })
        },
    );
    m.function(
        INSTANCE_TRANSFORM,
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(transform_of(&multimesh.instances[at], multimesh.flat))
            })
        },
    );
    m.function(
        SET_INSTANCE_COLOR,
        |eng: &Engine, (node, index, color): (NodeId, i64, Value)| {
            let color = four(&color, "an instance's colour")?;
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                multimesh.instances[at].color = color;
                Ok(())
            })
        },
    );
    m.function(
        INSTANCE_COLOR,
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].color))
            })
        },
    );
    m.function(
        SET_INSTANCE_CUSTOM_DATA,
        |eng: &Engine, (node, index, data): (NodeId, i64, Value)| {
            let data = four(&data, "an instance's custom data")?;
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                multimesh.instances[at].custom = data;
                Ok(())
            })
        },
    );
    m.function(
        INSTANCE_CUSTOM_DATA,
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].custom))
            })
        },
    );
}

/// The counts and every instance at once, as a list or Godot's flat buffer.
pub(crate) fn install_multimesh_list_api(m: &mut Module<Engine>, handle: &Handle) {
    let on = handle.on;
    m.describe(&[
        (SET_INSTANCE_COUNT, on, "(node: node, count: int)", "How many instances the node holds. Those below the count keep where they were; new ones are plain, at the node."),
        (INSTANCE_COUNT, on, "(node: node) -> int", "How many instances the node holds."),
        (SET_VISIBLE_INSTANCE_COUNT, on, "(node: node, count: int)", "How many instances draw, from the first; -1 draws them all."),
        ("visible_instance_count", on, "(node: node) -> int", "How many instances draw; -1 is all of them."),
        (INSTANCES, on, "(node: node) -> [render::MultimeshInstance]", "Every instance as the `multimesh` asset spells it, so `assets.save` writes a scripted layout into a file."),
        (SET_INSTANCES, on, "(node: node, instances: [render::MultimeshInstance])", "Replace every instance with a list in the `multimesh` asset's shape."),
        (BUFFER, on, "(node: node) -> [float]", "Every instance as one flat list of floats in Godot's `MultiMesh.buffer` layout: 12 of transform in 3D or 8 in 2D, then 4 of colour and 4 of custom data."),
        (SET_BUFFER, on, "(node: node, buffer: [float])", "Replace every instance from one flat list in the layout `buffer` answers in, for a Godot port that sets `buffer`. Building the list in a script costs more than one `set_instance_transform` per instance."),
    ]);
    m.function(BUFFER, |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                buffer_of(&multimesh.instances, multimesh.flat)
                    .into_iter()
                    .map(|v| Value::Num(f64::from(v)))
                    .collect(),
            ))
        })
    });
    m.function(SET_BUFFER, |eng: &Engine, (node, list): (NodeId, Value)| {
        let Value::List(items) = list else {
            bail!("a buffer is a list of numbers, not a {}", list.type_name());
        };
        let floats = items
            .iter()
            .map(|item| match item {
                Value::Num(n) => Ok(*n as f32),
                Value::Int(n) => Ok(*n as f32),
                other => Err(anyhow!(
                    "a buffer holds numbers, not a {}",
                    other.type_name()
                )),
            })
            .collect::<Result<Vec<f32>>>()?;
        with_multimesh(eng, node, |multimesh| {
            multimesh.instances = instances_from_buffer(&floats, multimesh.flat, true, true)?;
            Ok(())
        })
    });
    m.function(
        SET_INSTANCE_COUNT,
        |eng: &Engine, (node, count): (NodeId, i64)| {
            let count = count_of(count)?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances.resize(count, Instance::default());
                Ok(())
            })
        },
    );
    m.function(INSTANCE_COUNT, |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(i64::try_from(multimesh.instances.len()).unwrap_or(i64::MAX))
        })
    });
    m.function(
        SET_VISIBLE_INSTANCE_COUNT,
        |eng: &Engine, (node, count): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                multimesh.visible_instance_count = count.max(-1);
                Ok(())
            })
        },
    );
    m.function("visible_instance_count", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| Ok(multimesh.visible_instance_count))
    });
    m.function(INSTANCES, |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                multimesh.instances.iter().map(instance_value).collect(),
            ))
        })
    });
    m.function(
        SET_INSTANCES,
        |eng: &Engine, (node, list): (NodeId, Value)| {
            let Value::List(rows) = list else {
                bail!("instances are a list, not a {}", list.type_name());
            };
            count_of(i64::try_from(rows.len()).unwrap_or(i64::MAX))?;
            let instances = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    Instance::from_table(index, &balaur_core::node_api::to_toml(row)?)
                })
                .collect::<Result<Vec<_>>>()?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances = instances;
                Ok(())
            })
        },
    );
}
