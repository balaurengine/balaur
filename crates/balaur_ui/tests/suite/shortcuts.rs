//! The `shortcuts` component: chords on a node, each raising
//! `shortcut_pressed` with its row's name.

use balaur_core::App;

use crate::pass::{Script, feed, run};

/// `Root` counts each name it hears in a field of its own.
const HEARS: &str = "pub fn heard(name: any, _node: Node) {\n\
     if name == \"save\" { self.save += 1; }\n\
     if name == \"later\" { self.later += 1; }\n\
     if name == \"step\" { self.step += 1; }\n}";

const FIELDS: &str = "save: int, later: int, step: int";

/// A `Keys` node under `parent` holding `rows`, its presses heard by `Root`.
fn keys(parent: &str, rows: &str, more: &str) -> String {
    format!(
        "[[nodes]]\nid = \"keys\"\nname = \"Keys\"\nparent = \"{parent}\"\n{more}\
         shortcuts = {{ rows = [{rows}] }}\n\
         bindings = {{ rows = [{{ event = \"emitted:shortcut_pressed\", action = \"call\", target = \"/Root\", value = \"heard\" }}] }}\n"
    )
}

fn count(app: &App, name: &str) -> Option<f64> {
    let root = balaur_core::scene::find_node(&app.engine.world(), app.engine.root(), "Root")
        .expect("the scene has a Root");
    crate::support::script_number(app, root, name)
}

fn press(app: &mut App, ctx: &egui::Context, key: egui::Key, modifiers: egui::Modifiers) {
    feed(
        app,
        ctx,
        vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }],
    );
    app.tick(balaur_core::DEFAULT_FIXED_DT);
}

#[test]
fn a_pressed_chord_raises_its_rows_name_and_a_later_row_with_it_never_fires() {
    let rows = "{ chord = \"cmd+KeyS\", name = \"save\" }, { chord = \"cmd+KeyS\", name = \"later\" }, { chord = \"F10\", name = \"step\" }";
    let (_dir, mut app, ctx, errors) = run(&Script {
        fields: FIELDS,
        methods: HEARS,
        scene: &keys("root", rows, ""),
        ..Script::default()
    });
    assert!(errors.is_empty(), "{errors:#?}");
    press(&mut app, &ctx, egui::Key::S, egui::Modifiers::COMMAND);
    press(&mut app, &ctx, egui::Key::F10, egui::Modifiers::NONE);
    assert_eq!(
        (
            count(&app, "save"),
            count(&app, "later"),
            count(&app, "step")
        ),
        (Some(1.0), Some(0.0), Some(1.0))
    );
    press(&mut app, &ctx, egui::Key::A, egui::Modifiers::NONE);
    assert_eq!(
        (count(&app, "save"), count(&app, "step")),
        (Some(1.0), Some(1.0)),
        "a key no row names fired one"
    );
}

#[test]
fn a_hidden_node_s_shortcuts_are_silent() {
    for (more, heard) in [("", Some(1.0)), ("visible = false\n", Some(0.0))] {
        let (_dir, mut app, ctx, errors) = run(&Script {
            fields: FIELDS,
            methods: HEARS,
            scene: &keys("root", "{ chord = \"F10\", name = \"step\" }", more),
            ..Script::default()
        });
        assert!(errors.is_empty(), "{errors:#?}");
        press(&mut app, &ctx, egui::Key::F10, egui::Modifiers::NONE);
        assert_eq!(count(&app, "step"), heard, "{more:?}");
    }
}

// The edited copy of a scene sits in a viewport the editor sizes to nothing
// outside play: its keys must not answer the shell's chords.
#[test]
fn shortcuts_in_a_viewport_answer_only_while_it_has_a_size() {
    // Under a widget inside it, and straight under the viewport itself.
    let cases = [
        ("panel", 0.0, Some(0.0)),
        ("panel", 200.0, Some(1.0)),
        ("screen", 0.0, Some(0.0)),
        ("screen", 200.0, Some(1.0)),
    ];
    for (holder, width, heard) in cases {
        let screen = format!(
            "[[nodes]]\nid = \"screen\"\nname = \"Screen\"\nparent = \"root\"\n\
             widget = {{ kind = \"viewport\", x = 0.0, y = 0.0, width = {width:.1}, height = 100.0 }}\n\
             [[nodes]]\nid = \"panel\"\nname = \"Panel\"\nparent = \"screen\"\n\
             widget = {{ kind = \"panel\" }}\n{}",
            keys(holder, "{ chord = \"F10\", name = \"step\" }", "")
        );
        let (_dir, mut app, ctx, errors) = run(&Script {
            fields: FIELDS,
            methods: HEARS,
            scene: &screen,
            ..Script::default()
        });
        assert!(errors.is_empty(), "{errors:#?}");
        press(&mut app, &ctx, egui::Key::F10, egui::Modifiers::NONE);
        assert_eq!(
            count(&app, "step"),
            heard,
            "under {holder}, a viewport {width} wide"
        );
    }
}
