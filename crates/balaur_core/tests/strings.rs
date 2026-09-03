//! Localization: what a key answers with, in which language, and how a count
//! picks its form.

use balaur_core::strings;
use balaur_core::{App, AppConfig};
use balaur_script::Value;

const EN: &str = r#"
"menu.play" = "Play"
"menu.greet" = "Hello, {name}"
"menu.items" = { one = "{n} item", other = "{n} items" }
"only.english" = "English only"
"#;

const RO: &str = r#"
"menu.play" = "Joacă"
"menu.items" = { one = "{n} obiect", few = "{n} obiecte", other = "{n} de obiecte" }
"#;

fn project(manifest_locale: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        format!("name = \"t\"\nmain_scene = \"main.toml\"\n{manifest_locale}"),
    )
    .unwrap();
    std::fs::write(dir.path().join("main.toml"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("strings")).unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join("strings").join(name), body).unwrap();
    }
    dir
}

fn app_in(dir: &std::path::Path) -> App {
    let mut app = App::new(AppConfig {
        project_root: dir.to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: None,
    })
    .unwrap();
    app.load_project().unwrap();
    app
}

fn n(count: i64) -> Vec<(String, Value)> {
    vec![("n".to_string(), Value::Int(count))]
}

#[test]
fn a_key_answers_in_the_locale_in_force() {
    let dir = project(
        "\n[locale]\ndefault = \"ro\"\nfallback = \"en\"\n",
        &[("en.toml", EN), ("ro.toml", RO)],
    );
    let app = app_in(dir.path());
    assert_eq!(strings::locale(&app.engine), "ro");
    assert_eq!(strings::tr(&app.engine, "menu.play", &[]), "Joacă");
    strings::set_locale(&app.engine, "en");
    assert_eq!(strings::tr(&app.engine, "menu.play", &[]), "Play");
}

/// One hop, to the language the game was written in.
#[test]
fn a_key_the_locale_lacks_falls_back() {
    let dir = project(
        "\n[locale]\ndefault = \"ro\"\nfallback = \"en\"\n",
        &[("en.toml", EN), ("ro.toml", RO)],
    );
    let app = app_in(dir.path());
    assert_eq!(
        strings::tr(&app.engine, "only.english", &[]),
        "English only"
    );
}

/// Visible in the game rather than blank: a missing string is a bug to
/// notice, and an empty label is a bug to miss.
#[test]
fn a_key_nothing_has_comes_back_as_itself() {
    let dir = project("", &[("en.toml", EN)]);
    let app = app_in(dir.path());
    assert_eq!(
        strings::tr(&app.engine, "nobody.wrote.this", &[]),
        "nobody.wrote.this"
    );
}

#[test]
fn an_argument_is_interpolated_by_name() {
    let dir = project("", &[("en.toml", EN)]);
    let app = app_in(dir.path());
    let args = vec![("name".to_string(), Value::Str("Vasilisa".into()))];
    assert_eq!(
        strings::tr(&app.engine, "menu.greet", &args),
        "Hello, Vasilisa"
    );
}

/// A placeholder nothing was passed for is left as it is, so the hole is
/// visible to whoever has to fill it.
#[test]
fn a_placeholder_with_no_argument_is_left_alone() {
    let dir = project("", &[("en.toml", EN)]);
    let app = app_in(dir.path());
    let args = vec![("other".to_string(), Value::Int(1))];
    assert_eq!(
        strings::tr(&app.engine, "menu.greet", &args),
        "Hello, {name}"
    );
}

#[test]
fn english_counts_one_and_the_rest() {
    let dir = project("", &[("en.toml", EN)]);
    let app = app_in(dir.path());
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(1)), "1 item");
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(0)), "0 items");
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(5)), "5 items");
}

/// Romanian has a third form, which is the reason plural selection cannot be
/// "n == 1" and be done with it.
#[test]
fn romanian_counts_one_few_and_the_rest() {
    let dir = project(
        "\n[locale]\ndefault = \"ro\"\n",
        &[("en.toml", EN), ("ro.toml", RO)],
    );
    let app = app_in(dir.path());
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(1)), "1 obiect");
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(3)), "3 obiecte");
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(19)), "19 obiecte");
    assert_eq!(
        strings::tr(&app.engine, "menu.items", &n(20)),
        "20 de obiecte"
    );
    // 101..=119 counts as few again, which is the rule 100 does not reset.
    assert_eq!(
        strings::tr(&app.engine, "menu.items", &n(101)),
        "101 obiecte"
    );
}

/// A form the file does not carry falls to `other`, so a translator who wrote
/// two forms for a three-form language still reads sensibly.
#[test]
fn a_missing_plural_form_falls_to_other() {
    let dir = project(
        "\n[locale]\ndefault = \"ro\"\n",
        &[("ro.toml", "\"x\" = { one = \"unu\", other = \"multe\" }\n")],
    );
    let app = app_in(dir.path());
    assert_eq!(strings::tr(&app.engine, "x", &n(1)), "unu");
    assert_eq!(
        strings::tr(&app.engine, "x", &n(3)),
        "multe",
        "few is absent"
    );
}

/// A game may ship one language ahead of the rest, so a locale with no file
/// is empty rather than fatal.
#[test]
fn a_locale_with_no_file_is_empty_not_an_error() {
    let dir = project("\n[locale]\ndefault = \"de\"\n", &[("en.toml", EN)]);
    let app = app_in(dir.path());
    assert_eq!(
        strings::tr(&app.engine, "menu.play", &[]),
        "Play",
        "fell back"
    );
    assert_eq!(strings::tr(&app.engine, "absent", &[]), "absent");
}

#[test]
fn locales_lists_the_files_the_project_ships() {
    let dir = project("", &[("en.toml", EN), ("ro.toml", RO)]);
    let app = app_in(dir.path());
    assert_eq!(strings::locales(&app.engine), vec!["en", "ro"]);
}

/// `en-GB` is English for the purpose of counting.
#[test]
fn a_region_does_not_change_the_language_that_counts() {
    let dir = project("\n[locale]\ndefault = \"en-GB\"\n", &[("en-GB.toml", EN)]);
    let app = app_in(dir.path());
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(1)), "1 item");
    assert_eq!(strings::tr(&app.engine, "menu.items", &n(2)), "2 items");
}
