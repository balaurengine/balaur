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
