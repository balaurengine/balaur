//! A window that cannot present runs the frame all the same: its UI is laid
//! out where it was, and a dialog shut meanwhile gives the pointer back, as a
//! display that went to sleep under an editor showed when it did not.

use std::cell::RefCell;
use std::rc::Rc;

use balaur::{AppConfig, standard_app};
use balaur_core::{Engine, Stage};
use balaur_render::kiss3d_backend::NoPresent;

/// A dock down the left edge with a row of tools whose last one sits past a
/// spacer, the way the editor's docks keep their tools at the end of the
/// tabs, and a dialog over the middle, the way the command palette opens.
const SCENE: &str = r#"
[[nodes]]
id = "n_screen"
name = "Screen"

[[nodes]]
id = "n_dock"
name = "Dock"
parent = "n_screen"
[nodes.widget]
kind = "panel"
anchor = "fill_left"
width = 120
padding = 4

[[nodes]]
id = "n_tools"
name = "Tools"
parent = "n_dock"
[nodes.widget]
kind = "row"
height = 24

[[nodes]]
id = "n_tab"
name = "Tab"
parent = "n_tools"
[nodes.widget]
kind = "label"
text = "Scene"

[[nodes]]
id = "n_gap"
name = "Gap"
parent = "n_tools"
[nodes.widget]
kind = "label"
text = ""
grow = 1

[[nodes]]
id = "n_add"
name = "Add"
parent = "n_tools"
[nodes.widget]
kind = "button"
text = "+"

[[nodes]]
id = "n_palette"
name = "Palette"
parent = "n_screen"
[nodes.widget]
kind = "dialog"
text = "Palette"
width = 80
height = 40
[nodes.widget_window]
open = true
"#;

const WIDGETS: [&str; 4] = ["n_dock", "n_tools", "n_tab", "n_add"];
const DIALOG: &str = "n_palette";
const SIDE: u32 = 320;
/// Frames the run draws before the surface stops handing out frames, and
/// frames it runs after.
const PRESENTED: u32 = 20;
const REFUSED: u32 = 20;
/// The refused frame the dialog is shut on: the last pass that ended drew it.
const SHUT: u32 = 2;

/// What one frame saw: each dock widget's box, and whether a press inside
/// the dock and one past it, clear of the dialog, would go to the UI.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    boxes: Vec<Option<egui::Rect>>,
    dock_takes: bool,
    beyond_takes: bool,
}

fn seen(eng: &Engine) -> Seen {
    let world = eng.world();
    let boxes = WIDGETS
        .iter()
        .map(|id| balaur_core::ids::find(&world, eng.root(), id).and_then(balaur_ui::widget_rect))
        .collect();
    let mid = f32::from(u16::try_from(SIDE / 2).unwrap());
    Seen {
        boxes,
        dock_takes: balaur_ui::takes_point(eng, egui::pos2(30.0, mid)),
        beyond_takes: balaur_ui::takes_point(eng, egui::pos2(mid + 100.0, mid)),
    }
}

fn shut_dialog(eng: &Engine) {
    let dialog = balaur_core::ids::find(&eng.world(), eng.root(), DIALOG).expect("the dialog");
    let shut: toml::Value = toml::toml! { open = false }.into();
    balaur_core::components::patch(eng, dialog, "widget_window", &shut).unwrap();
}

/// The frame the surface refuses on, with the dialog open, and the last frame
/// of the run, the dialog shut while no frame could present.
fn run() -> (Option<Seen>, Option<Seen>) {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "project.toml",
        "[application]\nname = \"no_present\"\nmain_scene = \"scenes/main.toml\"\n",
    );
    write("scenes/main.toml", SCENE);
    let mut config = AppConfig::dev(dir.path().to_string_lossy().as_ref());
    config.watch = false;
    let mut app = standard_app(config).unwrap();
    app.load_project().unwrap();
    let before = Rc::new(RefCell::new(None));
    let after = Rc::new(RefCell::new(None));
    let (keep_before, keep_after) = (before.clone(), after.clone());
    let mut frame = 0;
    app.add_system(Stage::Last, move |eng, _| {
        frame += 1;
        if frame == PRESENTED {
            *keep_before.borrow_mut() = Some(seen(eng));
            eng.insert_resource(NoPresent);
        } else if frame == PRESENTED + SHUT {
            shut_dialog(eng);
        } else if frame == PRESENTED + REFUSED {
            *keep_after.borrow_mut() = Some(seen(eng));
            eng.request_quit();
        }
    });
    balaur_render::kiss3d_backend::run_offscreen(app, "no_present", SIDE, SIDE).unwrap();
    let (before, after) = (before.borrow().clone(), after.borrow().clone());
    (before, after)
}

#[test]
fn a_window_that_cannot_present_keeps_its_layout_and_hears_a_dialog_shut() {
    if !crate::film::gpu_present() {
        eprintln!("skipped: no GPU adapter");
        return;
    }
    let (before, after) = run();
    let before = before.expect("the run reached the frame the surface refused on");
    let after = after.expect("the run reached its last frame");
    let add = before.boxes[3].expect("the dock's tool drew while presenting");
    assert!(
        add.width() > 8.0 && add.height() > 8.0,
        "the tool has room to draw: {add:?}"
    );
    assert!(before.dock_takes, "a press inside the dock goes to the UI");
    assert!(before.beyond_takes, "and the open dialog takes one past it");
    assert_eq!(
        after.boxes, before.boxes,
        "frames that could not present moved the layout"
    );
    assert!(after.dock_takes, "the dock still takes its press");
    assert!(
        !after.beyond_takes,
        "shut while the window could not present, the dialog gives the point back"
    );
}
