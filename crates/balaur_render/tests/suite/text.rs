//! `text2d` as a script writes it with the words `render` names.

use balaur::{AppConfig, standard_app};

/// The node's `text2d` after `body` ran as its `init`.
fn text_after(body: &str) -> Option<toml::Value> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"t\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"n\"\nname = \"N\"\nscript = { source = \"scripts/s.rn\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/s.rn"),
        format!("pub fn init(this) {{\n{body}\n}}\n"),
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let node = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "N")?;
    balaur_core::components::get(&app.engine, node, "text2d")
}

fn word(text: &toml::Value, key: &str) -> Option<String> {
    text.get(key)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

#[test]
fn a_text_block_written_with_render_constants_reads_back_their_words() {
    let text = text_after(
        r#"this.node.set_component("text2d", #{ text: "hi", text_align: render::ALIGN_END, font_style: render::FONT_ITALIC });"#,
    )
    .expect("the script wrote a text2d");
    assert_eq!(word(&text, "text_align").as_deref(), Some("end"));
    assert_eq!(word(&text, "font_style").as_deref(), Some("italic"));
}

#[test]
fn a_text_block_that_names_no_alignment_is_centred_and_upright() {
    let text = text_after(r#"this.node.set_component("text2d", #{ text: "hi" });"#)
        .expect("the script wrote a text2d");
    assert_eq!(word(&text, "text_align").as_deref(), Some("center"));
    assert_eq!(word(&text, "font_style").as_deref(), Some("normal"));
}

/// A text component written whole by `params`, read back through `get`.
fn text_component(kind: &str, params: &str) -> toml::Value {
    let mut app = balaur_core::App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sign", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    balaur_core::components::add(&app.engine, node, kind, Some(&params))
        .unwrap_or_else(|why| panic!("{kind} takes {params}: {why:#}"));
    balaur_core::components::get(&app.engine, node, kind).expect("it reads back")
}

#[test]
fn a_text_block_takes_the_alpha_cutoff_a_material_spells() {
    for kind in ["text2d", "text3d"] {
        let text = text_component(kind, "text = \"hi\"\nalpha_cutoff = 0.5");
        assert_eq!(text["alpha_cutoff"].as_float(), Some(0.5), "{kind}");
    }
}

#[test]
fn a_3d_text_block_is_on_every_layer_until_it_names_some() {
    let text = text_component("text3d", "text = \"hi\"");
    assert_eq!(text["render_layers"].as_integer(), Some(-1));
    assert_eq!(text["light_layers"].as_integer(), Some(-1));
    assert_eq!(text["cast_shadow"].as_bool(), Some(true));
    let text = text_component(
        "text3d",
        "text = \"hi\"\nrender_layers = 2\nlight_layers = 4\ncast_shadow = false",
    );
    assert_eq!(text["render_layers"].as_integer(), Some(2));
    assert_eq!(text["light_layers"].as_integer(), Some(4));
    assert_eq!(text["cast_shadow"].as_bool(), Some(false));
}

#[test]
fn a_text_block_takes_every_shaping_key_and_reads_it_back() {
    let text = text_component(
        "text2d",
        "text = \"hi\"\nfont_style = \"oblique\"\nunderline = \"single\"\noverline = true\ntext_align = \"justify\"\ntruncate = true\ntruncate_at = \"start\"\nmax_lines = 2\nshaping = \"simple\"\nhinting = \"off\"\ntab_width = 4\nfont_features = [\"smcp\"]",
    );
    assert_eq!(word(&text, "font_style").as_deref(), Some("oblique"));
    assert_eq!(word(&text, "underline").as_deref(), Some("single"));
    assert_eq!(word(&text, "text_align").as_deref(), Some("justify"));
    assert_eq!(word(&text, "truncate_at").as_deref(), Some("start"));
    assert_eq!(word(&text, "shaping").as_deref(), Some("simple"));
    assert_eq!(word(&text, "hinting").as_deref(), Some("off"));
    assert_eq!(
        text.get("overline").and_then(toml::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        text.get("max_lines").and_then(toml::Value::as_integer),
        Some(2)
    );
    assert_eq!(
        text.get("tab_width").and_then(toml::Value::as_integer),
        Some(4)
    );
    assert_eq!(
        text.get("font_features")
            .and_then(toml::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn a_decoration_colour_reads_back_as_written() {
    let text = text_component(
        "text2d",
        "text = \"hi\"\nunderline = \"single\"\nunderline_color = [1, 0.25, 0.25, 1]",
    );
    let colour: Vec<f64> = text["underline_color"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_float().unwrap())
        .collect();
    assert_eq!(colour, [1.0, 0.25, 0.25, 1.0]);
}
