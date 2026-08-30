// Exact equality is the assertion: a determinism test that tolerated drift
// would not be testing determinism.
#![allow(clippy::float_cmp)]

//! Tests for the deterministic scripting layer: fastcall routing, libm-backed
//! math, and the seeded RNG.

use balaur_core::{App, AppConfig};
use balaur_script_luau::mlua::chunk::ChunkMode;

fn make_app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "name = \"t\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), "").unwrap();
    let app = App::new(AppConfig {
        project_root: dir.path().to_path_buf(),
        pack: None,
        watch: false,
        script_args: Vec::new(),
        script_backend: Some(balaur_script_luau::factory()),
    })
    .unwrap();
    (app, dir)
}

/// Compile a chunk with the engine's compiler configuration and evaluate it.
fn eval_compiled<T: balaur_script_luau::mlua::FromLuaMulti>(app: &App, src: &str) -> T {
    let lua = balaur_script_luau::lua_of(&app.engine);
    let bytecode = balaur_script_luau::compiler().compile(src).unwrap();
    lua.load(bytecode.as_slice())
        .set_mode(ChunkMode::Binary)
        .eval()
        .unwrap()
}

/// The Luau compiler normally turns `math.sin(x)` into a fastcall that
/// bypasses the global `math` table, which would silently reintroduce the
/// platform libm. Prove the engine's compiler configuration routes through
/// the table: rebind `math.sin` to a sentinel and check that fully compiled
/// (O2) code sees it.
#[test]
fn math_fastcalls_are_routed_through_the_global_table() {
    let (app, _dir) = make_app();
    let lua = balaur_script_luau::lua_of(&app.engine);
    lua.load("math.sin = function(x) return 42 end")
        .exec()
        .unwrap();
    let v: f64 = eval_compiled(
        &app,
        "local s = 0 for i = 1, 3 do s += math.sin(i) end return s",
    );
    assert_eq!(v, 126.0, "fastcall bypassed the rebound math.sin");
}

/// `math.sin` and friends must be the pure-Rust libm implementations, bit
/// for bit.
#[test]
fn math_functions_are_libm_backed() {
    let (app, _dir) = make_app();
    for (expr, expected) in [
        ("math.sin(0.5)", libm::sin(0.5)),
        ("math.cos(1.25)", libm::cos(1.25)),
        ("math.exp(2.5)", libm::exp(2.5)),
        ("math.log(7.5)", libm::log(7.5)),
        ("math.pow(1.3, 4.7)", libm::pow(1.3, 4.7)),
        ("math.atan(0.3, -0.8)", libm::atan2(0.3, -0.8)),
    ] {
        let v: f64 = eval_compiled(&app, &format!("return {expr}"));
        assert_eq!(
            v.to_bits(),
            expected.to_bits(),
            "{expr} does not match libm"
        );
    }
}

/// The engine RNG stream is reproducible from a seed and shared with
/// `math.random`.
#[test]
fn rng_is_seeded_and_reproducible() {
    let sequence = |app: &App| -> Vec<f64> {
        eval_compiled(
            app,
            r"
            rng.seed(1234)
            local out = {}
            for i = 1, 4 do out[i] = rng.random() end
            out[5] = math.random()          -- same stream
            out[6] = math.random(10)        -- int in [1, 10]
            out[7] = math.random(-5, 5)     -- int in [-5, 5]
            out[8] = rng.int(0, 100)
            return out
            ",
        )
    };
    let (app1, _d1) = make_app();
    let (app2, _d2) = make_app();
    let a = sequence(&app1);
    let b = sequence(&app2);
    assert_eq!(a, b, "identical seeds must give identical streams");
    assert!(a[0] >= 0.0 && a[0] < 1.0);
    assert!((1.0..=10.0).contains(&a[5]) && a[5].fract() == 0.0);
    assert!((-5.0..=5.0).contains(&a[6]) && a[6].fract() == 0.0);
    assert!((0.0..=100.0).contains(&a[7]));

    // A fresh engine with no explicit seed is reproducible by construction.
    let unseeded = |app: &App| -> f64 { eval_compiled(app, "return math.random()") };
    let (app3, _d3) = make_app();
    let (app4, _d4) = make_app();
    assert_eq!(unseeded(&app3), unseeded(&app4));
}
