//! The scene mirror redraws what the frame's sync placed and what was written,
//! and only that: a square follows its parent through a move, a hide, a
//! reparent, a free and a restore, and a `local` one the restore leaves out
//! keeps where it went, on a real GPU. A machine with none skips.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::scene::{self, GlobalTransform};
use balaur_core::{Engine, Stage, components, snapshot, transform};

use crate::film::gpu_present;

/// Under the camera, a red square two units left of a parent at the origin,
/// an empty node three units right, and a `local` green square above.
const SCENE: &str = r#"
[[nodes]]
id = "n_view"
name = "View"

[nodes.camera2d]
pixels_per_unit = 20.0

[[nodes]]
id = "n_parent"
name = "Parent"
parent = "n_view"

[nodes.transform]
position = [0.0, 0.0, 0.0]

[[nodes]]
id = "n_square"
name = "Square"
parent = "n_parent"

[nodes.transform]
position = [-2.0, 0.0, 0.0]

[nodes.shape2d]
color = [1.0, 0.0, 0.0, 1.0]
shape = { kind = "rectangle", size = [1.0, 1.0] }

[[nodes]]
id = "n_other"
name = "Other"
parent = "n_view"

[nodes.transform]
position = [3.0, 0.0, 0.0]

[[nodes]]
id = "n_hud"
name = "Hud"
parent = "n_view"
tags = ["local"]

[[nodes]]
id = "n_gauge"
name = "Gauge"
parent = "n_hud"

[nodes.transform]
position = [0.0, 4.0, 0.0]

[nodes.shape2d]
color = [0.0, 1.0, 0.0, 1.0]
shape = { kind = "rectangle", size = [1.0, 1.0] }
"#;

const SIDE: u32 = 320;
/// Frames between one step and the next, so each shot is written before the
/// next is asked for.
const APART: u64 = 4;

/// One step: what is done to the tree, and where the square stands after it,
/// in world units along x, or `None` where nothing should draw it.
type Step = (&'static str, fn(&Engine, &mut Kept), Option<f32>);

#[derive(Default)]
struct Kept {
    taken: Option<snapshot::Checkpoint>,
}

fn node(eng: &Engine, path: &str) -> balaur_core::hecs::Entity {
    scene::find_node(&eng.world(), eng.root(), &format!("View/{path}")).unwrap()
}

fn move_x(eng: &Engine, path: &str, x: f64) {
    let at = toml::Value::Array(vec![
        toml::Value::Float(x),
        toml::Value::Float(0.0),
        toml::Value::Float(0.0),
    ]);
    components::set_property(eng, node(eng, path), transform::COMPONENT, "position", &at).unwrap();
}

const STEPS: &[Step] = &[
    (
        "as loaded",
        |eng, kept| kept.taken = Some(snapshot::capture(eng)),
        Some(-2.0),
    ),
    (
        "the parent moved",
        |eng, _| move_x(eng, "Parent", 4.0),
        Some(2.0),
    ),
    (
        "the parent hidden",
        |eng, _| {
            scene::set_visible(eng, node(eng, "Parent"), false);
        },
        None,
    ),
    (
        "shown, reparented and its new parent moved",
        |eng, _| {
            scene::set_visible(eng, node(eng, "Parent"), true);
            eng.reparent(node(eng, "Parent/Square"), node(eng, "Other"))
                .unwrap();
            move_x(eng, "Other", 0.0);
        },
        Some(-1.0),
    ),
    (
        "its parent freed",
        |eng, _| {
            eng.defer_free(node(eng, "Other"));
            move_x(eng, "Hud", 2.0);
        },
        None,
    ),
    (
        "restored",
        |eng, kept| snapshot::restore(eng, kept.taken.as_ref().unwrap()),
        Some(-2.0),
    ),
];

/// The mean column of the pixels `like` picks out, over the whole picture.
fn column_of(picture: &image::RgbaImage, like: fn([u8; 4]) -> bool) -> Option<f32> {
    let found: Vec<u32> = picture
        .enumerate_pixels()
        .filter(|(_, _, pixel)| like(pixel.0))
        .map(|(x, _, _)| x)
        .collect();
    (!found.is_empty()).then(|| found.iter().sum::<u32>() as f32 / found.len() as f32)
}

fn red([r, g, b, _]: [u8; 4]) -> bool {
    r > 150 && g < 90 && b < 90
}

fn green([r, g, b, _]: [u8; 4]) -> bool {
    g > 150 && r < 90 && b < 90
}

fn columns(pictures: &[image::RgbaImage]) -> Vec<Option<f32>> {
    pictures
        .iter()
        .map(|picture| column_of(picture, red))
        .collect()
}

/// Run the steps, one every `APART` frames, and a last one that writes the
/// square's world pose behind the sync's back after the sync ran: answers a
/// picture per step.
fn shots() -> Vec<image::RgbaImage> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("scenes")).unwrap();
    std::fs::write(
        dir.path().join("project.toml"),
        "[application]\nname = \"mirror\"\nmain_scene = \"scenes/main.toml\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("scenes/main.toml"), SCENE).unwrap();
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let frame = Rc::new(RefCell::new(0u64));
    let kept = Rc::new(RefCell::new(Kept::default()));
    let count = STEPS.len() as u64 + 1;
    let at = |step: u64| APART * (step + 1);
    {
        let frame = frame.clone();
        app.add_system(Stage::Update, move |eng, _| {
            let now = {
                let mut frame = frame.borrow_mut();
                *frame += 1;
                *frame
            };
            if let Some(step) = (0..STEPS.len() as u64).find(|&s| at(s) == now) {
                (STEPS[step as usize].1)(eng, &mut kept.borrow_mut());
            }
        });
    }
    let paths: Vec<_> = (0..count)
        .map(|step| dir.path().join(format!("step{step}.png")))
        .collect();
    {
        let paths = paths.clone();
        app.add_system(Stage::Last, move |eng, _| {
            let now = *frame.borrow();
            let last = STEPS.len() as u64;
            if now == at(last) {
                // Behind the sync's back: no change tick says it moved.
                let square = node(eng, "Parent/Square");
                eng.world()
                    .get::<&mut GlobalTransform>(square)
                    .unwrap()
                    .position
                    .x = 5.0;
            }
            if let Some(step) = (0..count).find(|&s| at(s) == now) {
                balaur_render::request_screenshot(eng, paths[step as usize].clone());
            }
            if now == at(count) {
                eng.request_quit();
            }
        });
    }
    balaur_render::kiss3d_backend::run_offscreen(app, "mirror", SIDE, SIDE).unwrap();
    paths
        .iter()
        .map(|path| image::open(path).unwrap().to_rgba8())
        .collect()
}

#[test]
fn a_square_follows_its_parent_through_every_change_and_nothing_else() {
    if !gpu_present() {
        return;
    }
    let pictures = shots();
    let found = columns(&pictures);
    let (Some(left), Some(right)) = (found[0], found[1]) else {
        panic!("the square was not drawn where it was loaded or moved: {found:?}");
    };
    let per_unit = (right - left) / 4.0;
    let column = |x: f32| left + (x + 2.0) * per_unit;
    for (step, (what, _, wanted)) in STEPS.iter().enumerate() {
        match (wanted, found[step]) {
            (None, None) => {}
            (Some(x), Some(got)) => assert!(
                (got - column(*x)).abs() < 2.0,
                "{what}: the square is at column {got}, not {}",
                column(*x)
            ),
            (wanted, got) => panic!("{what}: wanted the square at {wanted:?}, found {got:?}"),
        }
    }
    let Some(last) = found[STEPS.len()] else {
        panic!("the square vanished when its world pose was written behind the sync's back");
    };
    assert!(
        (last - column(-2.0)).abs() < 2.0,
        "a world pose written behind the sync's back was mirrored: column {last}"
    );
    for (step, picture) in pictures.iter().enumerate().skip(4) {
        let gauge = column_of(picture, green);
        assert!(
            gauge.is_some_and(|got| (got - column(2.0)).abs() < 2.0),
            "step {step}: the local gauge is not where it moved to: {gauge:?}"
        );
    }
}
