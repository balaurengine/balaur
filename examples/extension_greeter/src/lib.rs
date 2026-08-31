//! An extension: the same `Plugin` a module implements, built as a cdylib and
//! loaded at run time rather than linked in.

use balaur_core::Engine;
use balaur_plugin::{Manifest, Plugin, Registry};
use balaur_script::BindingsExt as _;

pub struct Greeter {
    manifest: Manifest,
}

impl Default for Greeter {
    fn default() -> Self {
        Self {
            manifest: Manifest::new("greeter", env!("CARGO_PKG_VERSION")),
        }
    }
}

pub struct GreetingCount(pub u32);

impl Plugin for Greeter {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut Registry<'_>) -> anyhow::Result<()> {
        reg.insert_resource(GreetingCount(0));
        let mut m = reg.script_module("greeter")?;
        m.function("greet", |eng: &Engine, name: String| {
            eng.resource::<GreetingCount>().borrow_mut().0 += 1;
            Ok(format!("hello, {name}"))
        });
        m.function("greetings", |eng: &Engine, ()| {
            let count = eng.resource::<GreetingCount>().borrow().0;
            Ok(i64::from(count))
        });
        m.constant(
            "VERSION",
            balaur_script::Value::Str(env!("CARGO_PKG_VERSION").into()),
        );
        Ok(())
    }
}

balaur_plugin::export_plugin!(Greeter);
