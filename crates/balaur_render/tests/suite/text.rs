//! `text2d` as a script writes it with the words `render` names.

use balaur::{AppConfig, standard_app};

/// The node's `text2d` after `body` ran as its `init`.
fn text_after(body: &str) -> Option<toml::Value> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"fixture\"\nmain_scene = \"main.toml\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("main.toml"),
        "[[nodes]]\nid = \"scenario\"\nname = \"Scenario\"\nscript = { source = \"scripts/scenario.bs\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("scripts/scenario.bs"),
        format!(
            "pub class Scenario {{ node: Node }}\nimpl Scenario {{\nfn init() {{\n{body}\n}}\n}}\n"
        ),
    )
    .unwrap();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    let node = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Scenario")?;
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
        r#"self.node.set_component("text2d", map().with("text", "hi").with("text_align", render::ALIGN_END).with("font_style", render::FONT_ITALIC));"#,
    )
    .expect("the script wrote a text2d");
    assert_eq!(word(&text, "text_align").as_deref(), Some("end"));
    assert_eq!(word(&text, "font_style").as_deref(), Some("italic"));
}

#[test]
fn a_text_block_that_names_no_alignment_is_centred_and_upright() {
    let text = text_after(r#"self.node.set_component("text2d", map().with("text", "hi"));"#)
        .expect("the script wrote a text2d");
    assert_eq!(word(&text, "text_align").as_deref(), Some("center"));
    assert_eq!(word(&text, "font_style").as_deref(), Some("normal"));
}

/// A text component written whole by `params`, read back through `get`.
fn text_component(kind: &str, params: &str) -> toml::Value {
    text_component_in(kind, params).1
}

/// The same, with the app it was written in.
fn text_component_in(kind: &str, params: &str) -> (balaur_core::App, toml::Value) {
    let mut app = balaur_core::App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sign", root);
    let params: toml::Value = toml::from_str(params).unwrap();
    balaur_core::components::add(&app.engine, node, kind, Some(&params))
        .unwrap_or_else(|why| panic!("{kind} takes {params}: {why:#}"));
    let read = balaur_core::components::get(&app.engine, node, kind).expect("it reads back");
    (app, read)
}

/// A text block's cutoff is its material's `[surface]`: a cutout material
/// written inline is the one it draws with.
#[test]
fn a_text_block_takes_the_alpha_cutoff_its_material_spells() {
    for kind in ["text2d", "text3d"] {
        let mut app = balaur_core::App::new(AppConfig::bare(".")).unwrap();
        balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default()).unwrap();
        let root = app.engine.root();
        let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sign", root);
        let params: toml::Value = toml::from_str(
            "text = \"hi\"\nmaterial = { surface = { alpha = \"mask\", alpha_cutoff = 0.5 } }",
        )
        .unwrap();
        balaur_core::components::add(&app.engine, node, kind, Some(&params)).unwrap();
        let text = balaur_core::components::get(&app.engine, node, kind).unwrap();
        let named = text["material"].as_str().unwrap();
        let surface = balaur_render::material::flat_surface_of(&app.engine, named);
        assert!((surface.cutoff() - 0.5).abs() < 1e-6, "{kind}");
    }
}

#[test]
fn a_3d_text_block_is_on_every_layer_until_it_names_some() {
    let text = text_component("text3d", "text = \"hi\"");
    let every = toml::Value::Array(Vec::new());
    assert_eq!(text["render_layers"], every, "empty is every layer");
    assert_eq!(text["light_layers"], every);
    assert_eq!(text["cast_shadow"].as_bool(), Some(true));
    let text = text_component(
        "text3d",
        "text = \"hi\"\nrender_layers = [\"2\"]\nlight_layers = [\"3\"]\ncast_shadow = false",
    );
    let layer = |number: &str| toml::Value::Array(vec![toml::Value::String(number.into())]);
    assert_eq!(text["render_layers"], layer("2"));
    assert_eq!(text["light_layers"], layer("3"));
    assert_eq!(text["cast_shadow"].as_bool(), Some(false));
}

#[test]
fn a_text_block_takes_every_shaping_key_and_reads_it_back() {
    let text = text_component(
        "text2d",
        "text = \"hi\"\nfont_style = \"oblique\"\ntext_align = \"justify\"\ntruncate = true\ntruncate_at = \"start\"\nmax_lines = 2\ntab_width = 4\nfont = { shaping = \"simple\", hinting = \"off\", features = [\"smcp\"] }",
    );
    assert_eq!(word(&text, "font_style").as_deref(), Some("oblique"));
    assert_eq!(word(&text, "text_align").as_deref(), Some("justify"));
    assert_eq!(word(&text, "truncate_at").as_deref(), Some("start"));
    assert_eq!(
        text.get("max_lines").and_then(toml::Value::as_integer),
        Some(2)
    );
    assert_eq!(
        text.get("tab_width").and_then(toml::Value::as_integer),
        Some(4)
    );
    // How the font is shaped is the font's: the inline definition is filed
    // under the key the row reads back as.
    assert!(
        word(&text, "font").is_some_and(|key| key.starts_with("#!")),
        "{text}"
    );
}

/// The rows a text shaped its font with are the `font` asset's now: a scene
/// still writing one is refused by name.
#[test]
fn a_text_block_refuses_a_row_that_left_for_its_font() {
    let mut app = balaur_core::App::new(AppConfig::bare(".")).unwrap();
    balaur_plugin::load(&mut app, &mut balaur_render::RenderPlugin::default()).unwrap();
    let root = app.engine.root();
    let node = balaur_core::scene::spawn_node(&mut app.engine.world_mut(), "Sign", root);
    let params: toml::Value = toml::from_str("text = \"hi\"\nfont_family = \"mono\"").unwrap();
    let refused = balaur_core::components::add(&app.engine, node, "text2d", Some(&params))
        .expect_err("a chain word is no key of text2d");
    assert!(
        format!("{refused:#}").contains("font_family"),
        "{refused:#}"
    );
}

#[test]
fn a_text_style_written_inline_is_filed_with_its_rows() {
    let (app, text) = text_component_in(
        "text2d",
        "text = \"hi\"\ntext_style = { underline = \"single\", underline_color = [1, 0.25, 0.25, 1] }",
    );
    let key = word(&text, "text_style").expect("the row reads back as a reference");
    let style = balaur_core::assets::duplicate_definition(&app.engine, &key).unwrap();
    let colour: Vec<f64> = style["underline_color"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| balaur_core::components::as_f64(c).unwrap())
        .collect();
    assert_eq!(colour, [1.0, 0.25, 0.25, 1.0]);
    assert_eq!(word(&style, "underline").as_deref(), Some("single"));
}
