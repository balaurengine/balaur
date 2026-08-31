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

/// What `project_root` answers when the host's resources are keyed by a
/// `TypeId` this build does not compute the same way.
pub const NOT_VISIBLE: &str = "<not visible from the extension>";

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
        // Reads a host-inserted resource across the dylib boundary: the real
        // test that both compiled copies of balaur_core agree on the TypeId.
        m.function("project_root", |eng: &Engine, ()| {
            Ok(
                match eng.try_resource::<balaur_core::project::ProjectRoot>() {
                    Some(root) => root.borrow().0.display().to_string(),
                    None => NOT_VISIBLE.to_string(),
                },
            )
        });
        m.constant(
            "VERSION",
            balaur_script::Value::Str(env!("CARGO_PKG_VERSION").into()),
        );
        Ok(())
    }
}

balaur_plugin::export_plugin!(Greeter);
