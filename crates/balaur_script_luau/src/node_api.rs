//! The `Node` userdata exposed to Luau: a handle to a scene tree entity.
//!
//! The operations themselves live in `balaur_core::node_api`, declared once
//! for every language. This file is only the method-call sugar that makes
//! `node:position()` work.

use hecs::Entity;
use mlua::{MetaMethod, MultiValue, UserData, UserDataMethods, UserDataRef};

use balaur_core::engine::Engine;
use balaur_core::node_api::NODE_OPS;
use balaur_core::scene;

#[derive(Clone)]
pub struct NodeRef {
    pub entity: Entity,
    pub engine: Engine,
}

impl UserData for NodeRef {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for declared in NODE_OPS {
            let call = declared.call;
            methods.add_method(declared.name, move |lua, this, args: MultiValue| {
                let mut neutral = Vec::with_capacity(args.len() + 1);
                neutral.push(balaur_script::Value::Node(
                    balaur_core::node_id_of(this.entity).0,
                ));
                for v in &args {
                    neutral.push(crate::env::to_neutral(v)?);
                }
                let out = call(&this.engine, &neutral).map_err(mlua::Error::external)?;
                crate::env::from_neutral(lua, &this.engine, &out)
            });
        }

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
