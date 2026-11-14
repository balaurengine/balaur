//! A pack in pieces: a switch to a scene whose assets the tail holds waits
//! for the tail, and the tail's files are served once it arrives.

use balaur::project::ProjectFiles;
use balaur::{App, AppConfig, DEFAULT_FIXED_DT, Pack, standard_app};
use balaur_core::pack_tail::{TAIL, Tail, deliver};
use balaur_core::{scene, scene_switch};

const LATER: &str = "later.toml";
const FAR: &str = "art/far.txt";

fn head() -> Pack {
    let mut pack = Pack {
        manifest: "[application]\nname = \"pieces\"\nmain_scene = \"main.toml\"\n".into(),
        ..Pack::default()
    };
    pack.scenes.insert(
        "main.toml".into(),
        "[[nodes]]\nid = \"first\"\nname = \"First\"\n".into(),
    );
    pack.scenes.insert(
        LATER.into(),
        "[[nodes]]\nid = \"later\"\nname = \"Later\"\n".into(),
    );
    let tail = Tail {
        file: balaur::standalone::BUNDLED_TAIL.into(),
        waits: [LATER.to_string()].into(),
    };
    pack.assets.insert(TAIL.into(), tail.written());
    pack
}

fn booted() -> App {
    let mut app = standard_app(AppConfig::packed(head())).unwrap();
    app.load_project().unwrap();
    app
}

fn shows(app: &App, name: &str) -> bool {
    scene::find_node(&app.engine.world(), app.engine.root(), name).is_some()
}

fn served(app: &App, path: &str) -> bool {
    app.engine
        .resource::<ProjectFiles>()
        .borrow()
        .read(path)
        .is_ok()
}

#[test]
fn a_head_names_its_tail_and_the_scenes_that_wait() {
    let tail = Tail::of(&head()).expect("the head names a tail");
    assert_eq!(tail.file, balaur::standalone::BUNDLED_TAIL);
    assert!(tail.waits.contains(LATER));
}

#[test]
fn a_switch_to_a_scene_the_tail_holds_waits_for_the_tail() {
    let mut app = booted();
    scene_switch::request(&app.engine, LATER);
    for _ in 0..3 {
        app.tick(DEFAULT_FIXED_DT);
    }
    assert!(shows(&app, "First") && !shows(&app, "Later"));
    assert!(!served(&app, FAR));
    let mut tail = Pack::default();
    tail.assets.insert(FAR.into(), b"far".to_vec());
    deliver(tail);
    app.tick(DEFAULT_FIXED_DT);
    assert!(
        shows(&app, "Later"),
        "the switch went ahead once the tail arrived"
    );
    assert!(served(&app, FAR));
}
