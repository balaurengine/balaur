//! `settings.*`: read and write every declared setting, and turn the result
//! back into the files it came from.
//!
//! The editor is the caller this exists for. It lists the pages, renders each
//! one from its schema exactly as it renders a component, and writes the two
//! files back — `project.toml` for what the game ships with, its own
//! preferences for what only this developer wants.

use balaur_script::{Bindings, BindingsExt, Value};

use crate::engine::Engine;
use crate::node_api::{from_toml, to_toml};
use crate::settings::{self, Scope};

pub fn install_settings_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Every setting the engine and its plugins declare, in pages. A page \
         names a category, the table it is stored under, whether it belongs \
         to the project or to this editor, and a schema in the same shape a \
         component uses — so a settings page renders with the same code an \
         inspector does. Values are held in memory until `project_toml` or \
         `editor_toml` turns them back into the file they came from.",
    );
    m.describe(&[
        ("pages", &[], "()", "Every declared page as `{ category, table, scope, schema }`, in registration order."),
        ("get", &[], "(table: string, key: string)", "One setting's value: what was set, else what the schema defaults to, else nil."),
        ("set", &[], "(table: string, key: string, value: any)", "Change one setting, in memory; write it out with `project_toml` or `editor_toml`."),
        ("load_project", &[], "(manifest: string)", "Read every value out of a project.toml's text, so a page starts from what the project says."),
        ("project_toml", &[], "(manifest: string)", "The manifest text these settings would write, starting from the given text so anything they do not describe survives."),
        ("editor_toml", &[], "()", "The editor-scope settings as TOML, for a preferences file."),
    ]);
    m.function("pages", |eng: &Engine, (): ()| {
        let registry = settings::pages(eng);
        let registry = registry.borrow();
        let pages: Vec<Value> = registry
            .0
            .iter()
            .map(|page| {
                Value::Map(vec![
                    ("category".into(), Value::Str(page.category.clone())),
                    ("table".into(), Value::Str(page.table.clone())),
                    (
                        "scope".into(),
                        Value::Str(
                            match page.scope {
                                Scope::Project => "project",
                                Scope::Editor => "editor",
                            }
                            .into(),
                        ),
                    ),
                    (
                        "schema".into(),
                        from_toml(&page.schema).unwrap_or(Value::Nil),
                    ),
                ])
            })
            .collect();
        Ok(Value::List(pages))
    });
    m.function("get", |eng: &Engine, (table, key): (String, String)| {
        Ok(settings::get(eng, &table, &key)
            .and_then(|v| from_toml(&v).ok())
            .unwrap_or(Value::Nil))
    });
    m.function(
        "set",
        |eng: &Engine, (table, key, value): (String, String, Value)| {
            settings::set(eng, &table, &key, to_toml(&value)?);
            Ok(Value::Nil)
        },
    );
    m.function("load_project", |eng: &Engine, manifest: String| {
        settings::load_project(eng, &manifest)?;
        Ok(Value::Nil)
    });
    m.function("project_toml", |eng: &Engine, manifest: String| {
        Ok(Value::Str(settings::project_toml(eng, &manifest)?))
    });
    m.function("editor_toml", |eng: &Engine, (): ()| {
        Ok(Value::Str(settings::editor_toml(eng)?))
    });
}
