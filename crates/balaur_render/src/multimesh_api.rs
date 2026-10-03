//! The script handle a `multimesh3d` or `multimesh2d` node is driven
//! through: one instance at a time, the counts, and every instance at once.
//! Split from `multimesh` for its length.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::components::as_f64;
use balaur_core::entity_of;
use balaur_script::{Bindings, BindingsExt as _, NodeId, Value};
use glamx::{Affine2, Affine3A, Mat4};

use crate::multimesh::{
    Instance, MAX_INSTANCES, MULTIMESH_2D, MULTIMESH_3D, MultiMesh, buffer_of,
    instances_from_buffer,
};
use crate::vocabulary::keys as k;

/// The node's multimesh, for a handle method to read or write.
fn with_multimesh<R>(
    eng: &Engine,
    node: NodeId,
    f: impl FnOnce(&mut MultiMesh) -> Result<R>,
) -> Result<R> {
    let entity = entity_of(node)?;
    let world = eng.world_mut();
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

/// Four floats from a colour or a list of numbers.
fn four(value: &Value, what: &str) -> Result<[f32; 4]> {
    match value {
        Value::Color(c) => Ok(*c),
        list @ Value::List(_) => crate::draw_2d::color_of(list),
        other => bail!(
            "{what} is a colour or a list of four numbers, not a {}",
            other.type_name()
        ),
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

const BOTH: &[&str] = &[MULTIMESH_3D, MULTIMESH_2D];

/// One instance at a time on `node.multimesh3d` and `node.multimesh2d`:
/// Godot's `MultiMesh` calls, readers without their `get_`.
pub(crate) fn install_multimesh_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_instance_transform", BOTH, "", "Place one instance with a `Transform3d`, or a `Transform2d` on a 2D node. A shear is kept, as the instance's `basis`."),
        ("set_instance_region", &[MULTIMESH_2D], "", "The rectangle of the texture one instance draws, as an origin and a size in texture pixels; a zero size draws all of it. Not drawn yet: `multimesh2d` draws through Balaur's own pipeline, which binds no rectangle per instance."),
        ("instance_region", &[MULTIMESH_2D], "", "One instance's texture rectangle as `[x, y, w, h]` in pixels, or nil when it draws the whole texture."),
        ("instance_transform", BOTH, "", "One instance's transform: a `Transform3d`, or a `Transform2d` on a 2D node."),
        ("set_instance_color", BOTH, "", "The colour one instance draws in."),
        ("instance_color", BOTH, "", "The colour one instance draws in."),
        ("set_instance_custom_data", BOTH, "", "Four floats a material's shader reads for one instance, as a colour or a list."),
        ("instance_custom_data", BOTH, "", "One instance's four floats of custom data, as a colour."),
    ]);
    m.function(
        "set_instance_transform",
        |eng: &Engine, (node, index, transform): (NodeId, i64, Value)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                place(&mut multimesh.instances[at], &transform)
            })
        },
    );
    m.function(
        "set_instance_region",
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
        "instance_region",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(multimesh.instances[at].region.map_or(Value::Nil, |r| {
                    Value::List(r.iter().map(|v| Value::Num(f64::from(*v))).collect())
                }))
            })
        },
    );
    m.function(
        "instance_transform",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(transform_of(&multimesh.instances[at], multimesh.flat))
            })
        },
    );
    m.function(
        "set_instance_color",
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
        "instance_color",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].color))
            })
        },
    );
    m.function(
        "set_instance_custom_data",
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
        "instance_custom_data",
        |eng: &Engine, (node, index): (NodeId, i64)| {
            with_multimesh(eng, node, |multimesh| {
                let at = slot(multimesh, index)?;
                Ok(Value::Color(multimesh.instances[at].custom))
            })
        },
    );
}

/// The counts and every instance at once, as a list or Godot's flat buffer.
pub(crate) fn install_multimesh_list_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_instance_count", BOTH, "", "How many instances the node holds. Those below the count keep where they were; new ones are plain, at the node."),
        ("instance_count", BOTH, "", "How many instances the node holds."),
        ("set_visible_instance_count", BOTH, "", "How many instances draw, from the first; -1 draws them all."),
        ("visible_instance_count", BOTH, "", "How many instances draw; -1 is all of them."),
        ("instances", BOTH, "", "Every instance as the `multimesh` asset spells it, so `assets.save` writes a scripted layout into a file."),
        ("set_instances", BOTH, "", "Replace every instance with a list in the `multimesh` asset's shape."),
        ("buffer", BOTH, "", "Every instance as one flat list of floats in Godot's `MultiMesh.buffer` layout: 12 of transform in 3D or 8 in 2D, then 4 of colour and 4 of custom data."),
        ("set_buffer", BOTH, "", "Replace every instance from one flat list in the layout `buffer` answers in, for a Godot port that sets `buffer`. Building the list in a script costs more than one `set_instance_transform` per instance."),
    ]);
    m.function("buffer", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                buffer_of(&multimesh.instances, multimesh.flat)
                    .into_iter()
                    .map(|v| Value::Num(f64::from(v)))
                    .collect(),
            ))
        })
    });
    m.function(
        "set_buffer",
        |eng: &Engine, (node, list): (NodeId, Value)| {
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
        },
    );
    m.function(
        "set_instance_count",
        |eng: &Engine, (node, count): (NodeId, i64)| {
            let count = count_of(count)?;
            with_multimesh(eng, node, |multimesh| {
                multimesh.instances.resize(count, Instance::default());
                Ok(())
            })
        },
    );
    m.function("instance_count", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(i64::try_from(multimesh.instances.len()).unwrap_or(i64::MAX))
        })
    });
    m.function(
        "set_visible_instance_count",
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
    m.function("instances", |eng: &Engine, node: NodeId| {
        with_multimesh(eng, node, |multimesh| {
            Ok(Value::List(
                multimesh.instances.iter().map(instance_value).collect(),
            ))
        })
    });
    m.function(
        "set_instances",
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
