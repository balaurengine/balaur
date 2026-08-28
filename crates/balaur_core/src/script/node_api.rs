//! The `Node` userdata exposed to Luau: a handle to a scene tree entity.

use glamx::{EulerRot, Quat, Vec3};
use hecs::Entity;
use mlua::{MetaMethod, UserData, UserDataMethods, UserDataRef};

use crate::engine::{Command, Engine};
use crate::scene::{self, Name, Parent, Transform};

#[derive(Clone)]
pub struct NodeRef {
    pub entity: Entity,
    pub engine: Engine,
}

impl NodeRef {
    fn with_transform<R>(&self, f: impl FnOnce(&mut Transform) -> R) -> mlua::Result<R> {
        let world = self.engine.world();
        let mut transform = world
            .get::<&mut Transform>(self.entity)
            .map_err(|_| mlua::Error::runtime("node is dead or has no transform"))?;
        Ok(f(&mut transform))
    }
}

impl UserData for NodeRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_valid", |_, this, ()| {
            Ok(this.engine.world().contains(this.entity))
        });
        methods.add_method("name", |_, this, ()| {
            let world = this.engine.world();
            Ok(world
                .get::<&Name>(this.entity)
                .map(|n| n.0.clone())
                .unwrap_or_default())
        });
        methods.add_method("set_name", |_, this, name: String| {
            let world = this.engine.world();
            if let Ok(mut n) = world.get::<&mut Name>(this.entity) {
                n.0 = name;
            }
            Ok(())
        });
        methods.add_method("path", |_, this, ()| {
            Ok(scene::node_path(&this.engine.world(), this.entity))
        });

        methods.add_method("position", |_, this, ()| {
            this.with_transform(|t| (t.position.x, t.position.y, t.position.z))
        });
        methods.add_method("set_position", |_, this, (x, y, z): (f32, f32, f32)| {
            this.with_transform(|t| t.position = Vec3::new(x, y, z))
        });
        methods.add_method("translate", |_, this, (x, y, z): (f32, f32, f32)| {
            this.with_transform(|t| t.position += Vec3::new(x, y, z))
        });
        methods.add_method("rotation_euler", |_, this, ()| {
            this.with_transform(|t| {
                let (yaw, pitch, roll) = t.rotation.to_euler(EulerRot::ZYX);
                (roll, pitch, yaw)
            })
        });
        methods.add_method(
            "set_rotation_euler",
            |_, this, (roll, pitch, yaw): (f32, f32, f32)| {
                this.with_transform(|t| {
                    t.rotation = Quat::from_euler(EulerRot::ZYX, yaw, pitch, roll)
                })
            },
        );
        methods.add_method("scale", |_, this, ()| {
            this.with_transform(|t| (t.scale.x, t.scale.y, t.scale.z))
        });
        methods.add_method("set_scale", |_, this, (x, y, z): (f32, f32, f32)| {
            this.with_transform(|t| t.scale = Vec3::new(x, y, z))
        });
        methods.add_method("global_scale", |_, this, ()| {
            let world = this.engine.world();
            let g = world
                .get::<&crate::scene::GlobalTransform>(this.entity)
                .map_err(|_| mlua::Error::runtime("node is dead"))?;
            Ok((g.scale.x, g.scale.y, g.scale.z))
        });
        methods.add_method("global_rotation_euler", |_, this, ()| {
            let world = this.engine.world();
            let g = world
                .get::<&crate::scene::GlobalTransform>(this.entity)
                .map_err(|_| mlua::Error::runtime("node is dead"))?;
            let (yaw, pitch, roll) = g.rotation.to_euler(glamx::EulerRot::ZYX);
            Ok((roll, pitch, yaw))
        });
        methods.add_method("global_position", |_, this, ()| {
            let world = this.engine.world();
            let g = world
                .get::<&crate::scene::GlobalTransform>(this.entity)
                .map_err(|_| mlua::Error::runtime("node is dead"))?;
            Ok((g.position.x, g.position.y, g.position.z))
        });

        methods.add_method("get_node", |_, this, path: String| {
            let world = this.engine.world();
            Ok(
                scene::find_node(&world, this.entity, &path).map(|entity| NodeRef {
                    entity,
                    engine: this.engine.clone(),
                }),
            )
        });
        methods.add_method("add_child", |_, this, name: String| {
            let mut world = this.engine.world_mut();
            let entity = scene::spawn_node(&mut world, &name, this.entity);
            Ok(NodeRef {
                entity,
                engine: this.engine.clone(),
            })
        });
        methods.add_method("parent", |_, this, ()| {
            let world = this.engine.world();
            Ok(world.get::<&Parent>(this.entity).ok().map(|p| NodeRef {
                entity: p.0,
                engine: this.engine.clone(),
            }))
        });
        methods.add_method("children", |lua, this, ()| {
            let world = this.engine.world();
            let out = lua.create_table()?;
            if let Ok(children) = world.get::<&crate::scene::Children>(this.entity) {
                for (i, &child) in children.0.iter().enumerate() {
                    out.set(
                        i + 1,
                        NodeRef {
                            entity: child,
                            engine: this.engine.clone(),
                        },
                    )?;
                }
            }
            Ok(out)
        });

        // ---- components ------------------------------------------------
        methods.add_method(
            "add_component",
            |_, this, (name, params): (String, Option<mlua::Value>)| {
                let params = params
                    .map(|v| crate::script::tooling::lua_to_toml(&v))
                    .transpose()?;
                crate::components::add(&this.engine, this.entity, &name, params.as_ref())
                    .map_err(mlua::Error::external)
            },
        );
        // Same as add_component: apply is upsert. Present for readability.
        methods.add_method(
            "set_component",
            |_, this, (name, params): (String, mlua::Value)| {
                let params = crate::script::tooling::lua_to_toml(&params)?;
                crate::components::add(&this.engine, this.entity, &name, Some(&params))
                    .map_err(mlua::Error::external)
            },
        );
        methods.add_method("remove_component", |_, this, name: String| {
            crate::components::remove(&this.engine, this.entity, &name)
                .map_err(mlua::Error::external)
        });
        methods.add_method("get_component", |_, this, name: String| {
            Ok(crate::components::get(&this.engine, this.entity, &name)
                .map(crate::script::tooling::TomlToLua))
        });
        methods.add_method("has_component", |_, this, name: String| {
            Ok(crate::components::get(&this.engine, this.entity, &name).is_some())
        });
        methods.add_method("component_names", |_, this, ()| {
            Ok(crate::components::present_on(&this.engine, this.entity))
        });

        methods.add_method("script_path", |_, this, ()| {
            let world = this.engine.world();
            Ok(world
                .get::<&crate::scene::ScriptAttachment>(this.entity)
                .ok()
                .map(|a| a.path.clone()))
        });
        methods.add_method("attach_script", |_, this, path: String| {
            let host = this
                .engine
                .scripts()
                .ok_or_else(|| mlua::Error::runtime("script host not running"))?;
            host.attach(this.entity, &path)
                .map_err(mlua::Error::external)
        });
        methods.add_method("queue_free", |_, this, ()| {
            this.engine.push_command(Command::Free(this.entity));
            Ok(())
        });

        methods.add_meta_method(MetaMethod::Eq, |_, this, other: UserDataRef<NodeRef>| {
            Ok(this.entity == other.entity)
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Node({})",
                scene::node_path(&this.engine.world(), this.entity)
            ))
        });
    }
}
