//! Fonts as a text names them: a face file or a `font` asset, the theme's
//! default where it names none, and each project's faces kept to itself.

use balaur::{AppConfig, standard_app};
use balaur_core::App;
use balaur_text::{FontProject, Request};

/// The editor's faces, by file name.
fn face(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../editor/fonts")
            .join(name),
    )
    .unwrap()
}

/// A project at `dir` shipping `faces` under `fonts/` and a theme naming
/// `default` its default font.
fn project(dir: &std::path::Path, faces: &[&str], default: &str) {
    std::fs::write(
        dir.join("project.toml"),
        "[application]\nname = \"g\"\nmain_scene = \"main.toml\"\n\n[ui]\nsystem_fonts = false\ntheme = \"themes/look.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    for name in faces {
        std::fs::write(dir.join("fonts").join(name), face(name)).unwrap();
    }
    std::fs::write(
        dir.join("themes/look.toml"),
        format!("type = \"widget_theme\"\n\n[fonts]\ndefault = \"fonts/{default}\"\n"),
    )
    .unwrap();
}

fn app_at(dir: &std::path::Path) -> App {
    let mut config = AppConfig::dev(dir.to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    app.tick(1.0 / 60.0);
    app
}

/// How wide `text` measures in `font` of `project`'s faces.
fn width(app: &App, project: FontProject, font: &str, text: &str) -> f32 {
    balaur_text::load_font(&app.engine, project, font);
    let request = Request {
        project,
        font: font.to_string(),
        ..Request::new(text, 24.0)
    };
    balaur_text::shaper(&app.engine)
        .borrow_mut()
        .measure(&request)
        .x
}

/// A text naming no font draws in its theme's default, and one naming a
/// face draws in that face.
#[test]
fn a_text_draws_in_the_theme_s_default_font_or_the_face_it_names() {
    let dir = tempfile::tempdir().unwrap();
    project(
        dir.path(),
        &["SourceSans3-Regular.ttf", "JetBrainsMono-Regular.ttf"],
        "JetBrainsMono-Regular.ttf",
    );
    let app = app_at(dir.path());
    let mono = width(&app, FontProject::Host, "", "iiii");
    let sans = width(
        &app,
        FontProject::Host,
        "fonts/SourceSans3-Regular.ttf",
        "iiii",
    );
    assert!(
        mono > sans * 1.5,
        "the default measured {mono}, the face {sans}"
    );
    // A definition naming the face with settings of its own draws in it too.
    std::fs::write(
        dir.path().join("fonts/plain.toml"),
        "type = \"font\"\nsource = \"fonts/SourceSans3-Regular.ttf\"\nfeatures = [\"liga=0\"]\n",
    )
    .unwrap();
    let defined = width(&app, FontProject::Host, "fonts/plain.toml", "iiii");
    assert!((defined - sans).abs() < 0.01, "{defined} against {sans}");
}

/// The editor draws the game it plays in the game's faces and its own
/// panels in its own, and a face written under the game's `fonts/` is read
/// again once an asset moves.
#[test]
fn a_played_project_draws_in_its_own_faces_and_reads_them_again() {
    let editor = tempfile::tempdir().unwrap();
    let game = tempfile::tempdir().unwrap();
    project(
        editor.path(),
        &["SourceSans3-Regular.ttf"],
        "SourceSans3-Regular.ttf",
    );
    project(
        game.path(),
        &["JetBrainsMono-Regular.ttf"],
        "JetBrainsMono-Regular.ttf",
    );
    let app = app_at(editor.path());
    balaur_core::project::set_content_root(&app.engine, &game.path().to_string_lossy());
    let own = width(&app, FontProject::Host, "", "iiii");
    let played = width(&app, FontProject::Content, "", "iiii");
    assert!(
        played > own * 1.5,
        "the game measured {played}, the editor {own}"
    );
    // The game's theme takes another default; the watcher's word is an
    // asset generation.
    std::fs::write(
        game.path().join("fonts/SourceSans3-Regular.ttf"),
        face("SourceSans3-Regular.ttf"),
    )
    .unwrap();
    std::fs::write(
        game.path().join("themes/look.toml"),
        "type = \"widget_theme\"\n\n[fonts]\ndefault = \"fonts/SourceSans3-Regular.ttf\"\n",
    )
    .unwrap();
    balaur_core::assets::invalidate(&app.engine);
    let again = width(&app, FontProject::Content, "", "iiii");
    assert!(
        (again - own).abs() < 0.01,
        "the game's new default was not read: {again}"
    );
    // Playing nothing apart puts every text back in the engine's faces.
    balaur_core::project::set_content_root(&app.engine, "");
    assert!((width(&app, FontProject::Content, "", "iiii") - own).abs() < 0.01);
}
