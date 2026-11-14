// `event name;`: an item a file declares what its node sends with, read
// without building it, and what a checker reads of a file's names beside it.

use super::run;
use balaur_bs::analysis::{Kind, analyze};
use balaur_bs::runtime::Value;
use balaur_bs::{HostApi, Literal, Modules, Options, Pos, errors, events, names};
use std::rc::Rc;

const BALL: &str = "/// The ball left the table; the payload is the points it scored.
event scored;

event lost;

pub class Ball {
    node: Node,
    points: int,
}

impl Ball {
    pub fn on_scored(points: int) {
        self.points += points;
    }
}
";

fn options(path: &str) -> Options {
    Options {
        path: path.into(),
        ..Options::default()
    }
}

#[test]
fn an_event_item_is_read_with_the_doc_lines_above_it() {
    let found = events(BALL).unwrap();
    let read: Vec<(&str, &str, Pos)> = found
        .iter()
        .map(|event| (event.name.as_str(), event.doc.as_str(), event.pos))
        .collect();
    assert_eq!(
        read,
        [
            (
                "scored",
                "The ball left the table; the payload is the points it scored.",
                Pos { line: 2, col: 7 }
            ),
            ("lost", "", Pos { line: 4, col: 7 }),
        ]
    );
}

#[test]
fn a_doc_over_several_lines_is_one_description() {
    let src = "/// The first line,\n/// and the second.\nevent done;\n";
    assert_eq!(
        events(src).unwrap()[0].doc,
        "The first line, and the second."
    );
}

#[test]
fn a_file_with_events_compiles_and_runs_as_it_did() {
    assert_eq!(errors(BALL, &options("ball.bs")), []);
    let mut rt = run(BALL);
    let ball = rt.object("Ball", &[("points", Value::Int(1))]).unwrap();
    rt.call_method(&ball, "on_scored", &[Value::Int(4)])
        .unwrap();
    assert_eq!(rt.get(&ball, "points"), Some(Value::Int(5)));
}

#[test]
fn event_stays_a_name_everywhere_but_where_an_item_starts() {
    let mut rt = run("
        pub struct Queued { event: int }
        fn event() -> int { 2 }
        pub fn total(queued: [int]) -> int {
            let sum = event();
            for event in queued { sum += event; }
            let event = Queued { event: sum };
            event.event
        }
    ");
    let queued = Value::Array(vec![Value::Int(3), Value::Int(4)]);
    assert_eq!(rt.call("total", &[queued]).unwrap(), Value::Int(9));
}

#[test]
fn an_event_is_refused_twice_and_with_a_reach() {
    let said = |src: &str| events(src).unwrap_err().to_string();
    assert_eq!(
        said("event done;\nevent done;\n"),
        "2:7: event `done` is declared twice"
    );
    assert_eq!(
        said("pub event done;\n"),
        "1:5: an `event` takes no `pub` or `internal`: any script may hear it, found `event`"
    );
    assert_eq!(
        said("event done\nfn f() {}\n"),
        "2:1: expected `;`, found `fn`"
    );
}

#[test]
fn the_formatter_lays_an_event_out_and_leaves_it_laid() {
    let messy = "/// Sent once.\nevent   done ;\nevent\tlost;\n";
    let laid = "/// Sent once.\nevent done;\nevent lost;\n";
    assert_eq!(balaur_bs::fmt::format(messy).unwrap(), laid);
    assert_eq!(balaur_bs::fmt::format(laid).unwrap(), laid);
    let read = |src: &str| -> Vec<(String, String)> {
        let found = events(src).unwrap();
        found.into_iter().map(|e| (e.name, e.doc)).collect()
    };
    assert_eq!(read(laid), read(messy));
    assert_eq!(
        read(laid)[0],
        ("done".to_string(), "Sent once.".to_string())
    );
}

#[test]
fn an_editor_outlines_an_event_and_says_what_it_is() {
    let found = analyze(BALL, &HostApi::default());
    let outlined: Vec<(String, Kind, String)> = found
        .symbols()
        .into_iter()
        .filter(|symbol| symbol.kind == Kind::Event)
        .map(|symbol| (symbol.name, symbol.kind, symbol.detail))
        .collect();
    assert_eq!(
        outlined,
        [
            (
                "scored".to_string(),
                Kind::Event,
                "The ball left the table; the payload is the points it scored.".to_string()
            ),
            ("lost".to_string(), Kind::Event, String::new()),
        ]
    );
    assert_eq!(
        found.hover(2, 8).as_deref(),
        Some("/// The ball left the table; the payload is the points it scored.\nevent scored")
    );
    assert_eq!(found.hover(4, 8).as_deref(), Some("event lost"));
    let offered = analyze("ev\n", &HostApi::default()).complete(1, 3);
    assert!(
        offered
            .iter()
            .any(|item| item.label == "event" && item.kind == Kind::Keyword),
        "{offered:?}"
    );
}

const HEARER: &str = "use words;

const LOCAL: str = \"local_word\";

event scored;

pub class Hearer { node: Node }

impl Hearer {
    pub fn init() {
        events::listen(self.node, \"scored\", self.node);
        events::listen(self.node, words::LOST);
        let token = events::next(LOCAL);
        self.node.emit(input::WORD, 1);
        self.node.call(\"on_{LOCAL}_changed\");
        self.on_lost(token);
    }
    pub fn on_scored(points: int) {}
    fn on_lost(token: int) {}
}

fn helper() {}
";

fn hearer() -> balaur_bs::Names {
    let modules = Modules(Rc::new(|_: &str, name: &str| {
        (name == "words").then(|| {
            (
                "words.bs".to_string(),
                "pub const LOST: str = \"lost\";\n".to_string(),
            )
        })
    }));
    let host = HostApi {
        constants: vec![("input::WORD".into(), Literal::Str("engine_word".into()))],
        ..HostApi::default()
    };
    let options = Options {
        modules: Some(modules),
        host,
        ..options("scripts/hearer.bs")
    };
    names(HEARER, &options).unwrap()
}

#[test]
fn a_files_names_are_its_events_and_its_class_methods() {
    let found = hearer();
    let declared: Vec<&str> = found
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    assert_eq!(declared, ["scored"]);
    let methods: Vec<&str> = found
        .methods
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(methods, ["init", "on_scored", "on_lost"]);
    assert_eq!(found.methods[1].1.line, 18);
    assert_eq!(found.modules, ["words"]);
}

#[test]
fn a_call_is_read_with_each_name_a_literal_or_a_constant_spells() {
    let found = hearer();
    let read: Vec<String> = found
        .calls
        .iter()
        .map(|call| {
            let args: Vec<String> = call
                .args
                .iter()
                .map(|(at, text, _)| format!("{at}={text}"))
                .collect();
            let dot = if call.method { "." } else { "" };
            format!("{dot}{} {}", call.callee, args.join(" "))
        })
        .collect();
    assert_eq!(
        read,
        [
            "events::listen 1=scored",
            "events::listen 1=lost",
            "events::next 0=local_word",
            ".emit 0=engine_word",
        ]
    );
    assert_eq!(found.calls[0].args[0].2, Pos { line: 11, col: 35 });
}

#[test]
fn what_a_file_calls_and_spells_is_kept_for_a_method_named_elsewhere() {
    let found = hearer();
    assert!(found.called.iter().any(|name| name == "on_lost"));
    assert!(!found.called.iter().any(|name| name == "on_scored"));
    assert!(found.strings.iter().any(|text| text == "local_word"));
    assert_eq!(
        found.patterns,
        [["on_".to_string(), "_changed".to_string()]]
    );
}
