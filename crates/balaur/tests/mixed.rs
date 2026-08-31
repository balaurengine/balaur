//! Both languages in one project (`language = "mixed"`), calling each other
//! through `node:call` — names and neutral values crossing the seam, no
//! shared code required.

use balaur::{standard_app, AppConfig};

static LOG: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Boot a mixed project from `files`, tick until every `markers` entry shows
/// up in the log. Panics on any logged error or on timeout.
fn run_until(files: &[(&str, &str)], markers: &[&str]) {
    let _guard = LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"m\"\nmain_scene = \"main.toml\"\nlanguage = \"mixed\"\n",
    )
    .unwrap();
    for (name, body) in files {
        std::fs::write(dir.path().join(name), body).unwrap();
    }

    balaur_core::logbuf::capture_for_test();
    balaur_core::logbuf::clear();
    let mut app = standard_app(AppConfig::dev(dir.path().to_string_lossy().as_ref())).unwrap();
    app.load_project().unwrap();
    for _ in 0..400 {
        app.tick(1.0 / 60.0);
        let recent = balaur_core::logbuf::recent(50);
        let errors: Vec<_> = recent
            .iter()
            .filter(|e| e.level.eq_ignore_ascii_case("error"))
            .collect();
        assert!(errors.is_empty(), "a script logged errors: {errors:#?}");
        if markers
            .iter()
            .all(|m| recent.iter().any(|e| e.message.contains(m)))
        {
            return;
        }
    }
    panic!("the scripts never logged all of {markers:?}");
}

#[test]
fn lua_and_rune_scripts_call_each_other_in_one_scene() {
    run_until(
        &[
            (
                "main.toml",
                r#"
[[nodes]]
id = "s1"
name = "LuaService"
script = "scripts/service.luau"

[[nodes]]
id = "s2"
name = "RuneService"
script = "scripts/service.rn"

[[nodes]]
id = "c1"
name = "LuaConsumer"
script = "scripts/consumer.luau"

[[nodes]]
id = "c2"
name = "RuneConsumer"
script = "scripts/consumer.rn"
"#,
            ),
            (
                "scripts/service.luau",
                "local S = {}\nfunction S:sum(a, b) return a + b end\nreturn S\n",
            ),
            ("scripts/service.rn", "pub fn scaled(this, n) { n * 3 }\n"),
            (
                "scripts/consumer.luau",
                r#"
local S = {}
function S:init()
    local rune_service = scene.get_node("RuneService")
    log.info("mixed-lua-to-rune " .. rune_service:call("scaled", 7))
end
return S
"#,
            ),
            (
                "scripts/consumer.rn",
                r#"pub fn init(this) {
    let lua_service = scene::get_node("LuaService");
    log::info(`mixed-rune-to-lua ${lua_service.call("sum", 2, 3)}`);
}"#,
            ),
        ],
        &["mixed-lua-to-rune 21", "mixed-rune-to-lua 5"],
    );
}
