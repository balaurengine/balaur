//! The `code` kind: the gutter a node marks, and the line a click on it
//! reports. The editor draws itself with the same call through `ui::code_editor`,
//! so what is tested here is what a scene reaches.

#[allow(unused_imports, reason = "each suite uses part of the shared helpers")]
use crate::support::*;
use egui::pos2;

/// The fill of every circle and rect a pass painted, for the marks the gutter
/// leaves beside the code.
fn fills(out: &egui::FullOutput) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, into: &mut Vec<egui::Color32>) {
        match shape {
            egui::epaint::Shape::Circle(circle) => into.push(circle.fill),
            egui::epaint::Shape::Rect(rect) => into.push(rect.fill),
            egui::epaint::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, into)),
            _ => {}
        }
    }
    let mut painted = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut painted);
    }
    painted
}

/// The lines a `code` node names are what its gutter marks, in the colours
/// its theme names: a dot on a breakpoint, a fill across the current line.
#[test]
fn a_code_gutter_marks_the_lines_its_node_names() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/game.toml"),
        "type = \"widget_theme\"\n\n[colors]\nbreakpoint_color = \"#ff8800\"\ncurrent_fill = \"#00ff00\"\n",
    )
    .unwrap();
    let params = toml::toml! {
        kind = "code" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        theme = "themes/game.toml" text = "one\ntwo\nthree"
        widget_code = { breakpoints = ["2"], current_line = 3 }
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let painted = fills(&pass(&app, &ctx, vec![]));
    let wearing = |want: egui::Color32| painted.contains(&want);
    assert!(
        wearing(egui::Color32::from_rgb(0xff, 0x88, 0x00)),
        "the breakpoint dot is painted in the theme's colour"
    );
    assert!(
        wearing(egui::Color32::from_rgb(0x00, 0xff, 0x00)),
        "and the current line is filled in the theme's"
    );
}

/// A click on the gutter reports the line it landed on and nothing else: what
/// the mark means is the script's, as a breakpoint in one editor and a value
/// in another.
#[test]
fn a_code_gutter_click_reports_its_line() {
    let script = "pub class Owner { node: Node, hit: int }
impl Owner {
    pub fn on_hit(line: int) { self.hit = line; }
    pub fn hit() -> int { self.hit }
}
";
    let (_dir, mut app) = app_with_script("owner.bs", script);
    let root = app.engine.root();
    let owner = balaur::scene::spawn_node(&mut app.engine.world_mut(), "Owner", root);
    let host = app.engine.script_host().unwrap();
    host.attach(balaur::node_id_of(owner), "scripts/owner.bs")
        .unwrap();
    let params = toml::toml! {
        kind = "code" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        text = "one\ntwo\nthree"
        widget_code = { on_gutter = "on_hit" }
    };
    let entity = add_child_widget(&app, owner, "Code", &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let rect = balaur_ui::widget_rect(entity).expect("the editor drew");
    let at = pos2(rect.min.x + 4.0, rect.min.y + 4.0);
    pass(&app, &ctx, press(at, true));
    pass(&app, &ctx, press(at, false));
    consume_input(&mut app);
    assert_eq!(
        host.call_on(balaur::node_id_of(owner), "hit", &[]),
        Some(balaur_script::Value::Int(1)),
        "the click on the first gutter row reached the handler with its line"
    );
}

/// The colour of every glyph the shaper painted, by vertex.
fn glyph_colors(out: &egui::FullOutput) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, into: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                into.extend(mesh.vertices.iter().map(|v| v.color));
            }
            egui::Shape::Vec(inner) => inner.iter().for_each(|one| walk(one, into)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

/// The code is shaped as a label is, a glyph a letter, and only the gutter's
/// numbers go through egui's own text; so it stays at another scale.
#[test]
fn a_code_editor_draws_its_lines_through_the_shaper() {
    let (_dir, app) = app();
    let params = toml::toml! {
        kind = "code" x = 0.0 y = 0.0 width = 300.0 height = 120.0 text = "one\ntwo"
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let out = pass(&app, &ctx, vec![]);
    assert_eq!(glyphs(&out).len(), 6, "a glyph a letter of both lines");
    let drawn: Vec<String> = texts(&out).into_iter().map(|(text, _)| text).collect();
    assert!(
        drawn
            .iter()
            .all(|text| text.chars().all(|c| c.is_ascii_digit())),
        "egui's text draws only the gutter: {drawn:?}"
    );
    set_scale(&app, &ctx, 2.0);
    pass(&app, &ctx, vec![]);
    assert_eq!(
        glyphs(&pass(&app, &ctx, vec![])).len(),
        6,
        "and again at twice the scale"
    );
}

/// A keyword wears the theme's keyword colour, and the name after it the
/// identifier's.
#[test]
fn a_code_editor_colours_a_line_by_its_words() {
    let (dir, app) = app();
    std::fs::create_dir_all(dir.path().join("themes")).unwrap();
    std::fs::write(
        dir.path().join("themes/game.toml"),
        "type = \"widget_theme\"\n\n[colors]\nsyntax_keyword = \"#ff0000\"\nsyntax_identifier = \"#00ff00\"\n",
    )
    .unwrap();
    let params = toml::toml! {
        kind = "code" x = 0.0 y = 0.0 width = 300.0 height = 120.0
        theme = "themes/game.toml" text = "fn go"
    };
    add_widget(&app, &params.into());
    let ctx = egui::Context::default();
    settle(&app, &ctx);
    let colors = glyph_colors(&pass(&app, &ctx, vec![]));
    let wearing = |want: egui::Color32| colors.iter().filter(|c| **c == want).count();
    assert_eq!(
        wearing(egui::Color32::from_rgb(0xff, 0, 0)),
        8,
        "`fn`, four corners a glyph"
    );
    assert_eq!(wearing(egui::Color32::from_rgb(0, 0xff, 0)), 8, "`go`");
}
