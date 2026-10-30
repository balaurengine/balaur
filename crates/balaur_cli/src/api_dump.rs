//! `balaur api`: what scripts can reach, as JSON for the docs and the
//! language server.

use anyhow::Result;
use balaur::AppConfig;

/// Print what scripts can reach.
pub(crate) fn dump_api() -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&api_value()?)?);
    Ok(())
}

/// Boot a standard app in a scratch project and ask it what scripts can reach.
///
/// The engine is asked, not the source: constants like `input::KEY_SPACE` are
/// derived at registration, so parsing Rust would miss them.
pub(crate) fn api_value() -> Result<serde_json::Value> {
    let dir = std::env::temp_dir().join("balaur-api-probe");
    std::fs::create_dir_all(dir.join("scenes"))?;
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"api\"\nmain_scene = \"scenes/main.toml\"\n",
    )?;
    std::fs::write(
        dir.join("scenes/main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"Root\"\n",
    )?;

    let mut app = balaur::standard_app(AppConfig::dev(dir.to_string_lossy().as_ref()))?;
    // The editor's own modules are registered by this binary rather than by
    // the engine, so the probe loads them or the reference would list none.
    #[cfg(not(target_family = "wasm"))]
    balaur_plugin::load_all(&mut app, &mut crate::own_modules(&dir))?;
    app.load_project()?;
    let mut api = serde_json::json!({"modules": [], "types": [], "builtins": {}});
    #[cfg(any(feature = "bs", feature = "bs-jit", feature = "bs-web"))]
    if let Some(host) = balaur::script_bs::BsHost::of(&app.engine) {
        api["modules"] = balaur::file_api::to_json(&host.reference())?;
        api["types"] = value_types();
        api["builtins"] = builtins();
        api["bs"] = bs_api(&host.api());
        api["bs"]["unread"] = serde_json::to_value(host.unread_records())?;
    }
    // Component schemas ride along, so docs and tools read one probe, with
    // the order each writes its properties in, which is the inspector's.
    let schemas = balaur::components::schemas(&app.engine);
    let components: std::collections::BTreeMap<&str, serde_json::Value> = schemas
        .iter()
        .map(|(name, schema)| {
            let specs: &toml::Value = schema;
            Ok((name.as_str(), serde_json::to_value(specs)?))
        })
        .collect::<Result<_>>()?;
    api["components"] = serde_json::to_value(components)?;
    let component_order: std::collections::BTreeMap<&str, &[String]> = schemas
        .iter()
        .map(|(name, schema)| (name.as_str(), schema.order()))
        .collect();
    api["component_order"] = serde_json::to_value(component_order)?;
    // What each component is for, and the facets it belongs to, so the
    // reference can describe and group them.
    let component_docs: std::collections::BTreeMap<String, &'static str> = app
        .engine
        .try_resource::<balaur::components::ComponentRegistry>()
        .map(|registry| {
            registry
                .borrow()
                .iter()
                .map(|(name, def)| (name.to_string(), def.doc))
                .collect()
        })
        .unwrap_or_default();
    api["component_docs"] = serde_json::to_value(component_docs)?;
    let component_tags: std::collections::BTreeMap<String, Vec<&'static str>> = app
        .engine
        .try_resource::<balaur::components::ComponentRegistry>()
        .map(|registry| {
            registry
                .borrow()
                .iter()
                .map(|(name, def)| (name.to_string(), def.tags.to_vec()))
                .collect()
        })
        .unwrap_or_default();
    api["component_tags"] = serde_json::to_value(component_tags)?;
    // What each component announces from its node, as `(name, payload)`.
    let component_events: std::collections::BTreeMap<String, Vec<(&'static str, &'static str)>> =
        app.engine
            .try_resource::<balaur::components::ComponentRegistry>()
            .map(|registry| {
                registry
                    .borrow()
                    .iter()
                    .filter(|(_, def)| !def.events.is_empty())
                    .map(|(name, def)| (name.to_string(), def.events.to_vec()))
                    .collect()
            })
            .unwrap_or_default();
    api["component_events"] = serde_json::to_value(component_events)?;
    // Every method the engine calls on a script by name.
    api["hooks"] = serde_json::to_value(balaur::hooks::REFERENCE)?;
    api["node_events"] = serde_json::to_value(balaur::node_api::NODE_EVENTS)?;
    let asset_types: std::collections::BTreeMap<String, serde_json::Value> = app
        .engine
        .try_resource::<balaur::assets::AssetTypeRegistry>()
        .map(|registry| {
            registry
                .borrow()
                .0
                .iter()
                .map(|(name, t)| {
                    let mut described = serde_json::json!({"directory": t.directory, "doc": t.doc});
                    // A record's rows, in the order an Inspector shows them.
                    if let Some(schema) = &t.schema {
                        let specs: &toml::Value = schema;
                        described["schema"] = serde_json::to_value(specs).unwrap_or_default();
                        described["order"] =
                            serde_json::to_value(schema.order()).unwrap_or_default();
                    }
                    (name.clone(), described)
                })
                .collect()
        })
        .unwrap_or_default();
    api["asset_types"] = serde_json::to_value(asset_types)?;
    Ok(api)
}

/// The value types every script holds, each with its fields.
#[cfg(any(feature = "bs", feature = "bs-jit", feature = "bs-web"))]
fn value_types() -> serde_json::Value {
    balaur::script_bs::value_types()
        .iter()
        .map(|(name, fields)| serde_json::json!({"name": name, "fields": fields}))
        .collect()
}

/// The functions every script calls without a module, with their signatures.
#[cfg(any(feature = "bs", feature = "bs-jit", feature = "bs-web"))]
fn builtins() -> serde_json::Value {
    balaur::script_bs::builtins()
        .iter()
        .map(|(name, signature)| ((*name).to_string(), serde_json::json!(signature)))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// The engine as a `.bs` script compiles against it: each function's
/// signature and how many arguments a call must pass, each constant, and
/// each record's fields.
#[cfg(any(feature = "bs", feature = "bs-jit", feature = "bs-web"))]
fn bs_api(api: &balaur::script_bs::HostApi) -> serde_json::Value {
    use balaur::script_bs::Literal;
    let functions: serde_json::Map<String, serde_json::Value> = api
        .functions
        .iter()
        .map(|f| {
            let row = serde_json::json!({"signature": f.text(), "least": f.least()});
            (f.key.clone(), row)
        })
        .collect();
    let constants: serde_json::Map<String, serde_json::Value> = api
        .constants
        .iter()
        .map(|(k, v)| {
            let v = match v {
                Literal::Int(x) => serde_json::json!(x),
                Literal::Float(x) => serde_json::json!(x),
                Literal::Bool(b) => serde_json::json!(b),
                Literal::Str(t) => serde_json::json!(t),
            };
            (k.clone(), v)
        })
        .collect();
    let structs: serde_json::Map<String, serde_json::Value> = api
        .structs
        .iter()
        .map(|(k, fields)| {
            let fields: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|(n, t)| (n.clone(), serde_json::json!(t.to_string())))
                .collect();
            (k.clone(), serde_json::Value::Object(fields))
        })
        .collect();
    serde_json::json!({"functions": functions, "constants": constants, "structs": structs})
}
