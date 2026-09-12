//! `balaur api`: what scripts can reach, as JSON for the docs and the
//! language server.

use anyhow::Result;
use balaur::AppConfig;

#[cfg(not(target_family = "wasm"))]
use crate::{export_api, import_api};

/// Boot a standard app in a scratch project and print what scripts can reach.
///
/// The engine is asked, not the source: constants like `input.KEY_SPACE` are
/// derived at registration, so parsing Rust would miss them.
pub(crate) fn dump_api() -> Result<()> {
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
    // `export` and `import` are the editor's, registered by this binary rather
    // than by the engine, so the probe loads both or the reference would list
    // neither.
    #[cfg(not(target_family = "wasm"))]
    balaur_plugin::load(&mut app, &mut export_api::ExportPlugin::new(dir.clone()))?;
    #[cfg(not(target_family = "wasm"))]
    balaur_plugin::load(&mut app, &mut import_api::ImportPlugin::new(dir.clone()))?;
    app.load_project()?;
    let host = balaur::rune::rune_of(&app.engine);
    let mut api: serde_json::Value = serde_json::from_str(&balaur::rune::api_json(&host)?)?;
    // Component schemas ride along, so docs and tools read one probe.
    let components: std::collections::BTreeMap<String, serde_json::Value> =
        balaur::components::schemas(&app.engine)
            .into_iter()
            .map(|(name, schema)| Ok((name, serde_json::to_value(schema)?)))
            .collect::<Result<_>>()?;
    api["components"] = serde_json::to_value(components)?;
    // What each component is for, and the facets it belongs to, so the
    // reference can describe and group them.
    let component_docs: std::collections::BTreeMap<String, &'static str> = app
        .engine
        .try_resource::<balaur::components::ComponentRegistry>()
        .map(|registry| {
            registry
                .borrow()
                .0
                .iter()
                .map(|(name, def)| (name.clone(), def.doc))
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
                .0
                .iter()
                .map(|(name, def)| (name.clone(), def.tags.to_vec()))
                .collect()
        })
        .unwrap_or_default();
    api["component_tags"] = serde_json::to_value(component_tags)?;
    let asset_types: std::collections::BTreeMap<String, serde_json::Value> = app
        .engine
        .try_resource::<balaur::assets::AssetTypeRegistry>()
        .map(|registry| {
            registry
                .borrow()
                .0
                .iter()
                .map(|(name, t)| {
                    (
                        name.clone(),
                        serde_json::json!({"directory": t.directory, "doc": t.doc}),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    api["asset_types"] = serde_json::to_value(asset_types)?;
    println!("{}", serde_json::to_string_pretty(&api)?);
    Ok(())
}
