use std::sync::Arc;

use super::*;
use crate::{Request, TextState};

fn editing(text: &str) -> Editing {
    Editing::new(text, true)
}

fn press(e: &mut Editing, change: &Change, times: usize) {
    for _ in 0..times {
        e.change(change);
    }
}

#[test]
fn deleting_back_takes_a_whole_grapheme() {
    let mut e = editing("e\u{301}👨\u{200d}👩\u{200d}👧x");
    press(&mut e, &Change::DeleteBack, 1);
    assert_eq!(e.text(), "e\u{301}👨\u{200d}👩\u{200d}👧");
    press(&mut e, &Change::DeleteBack, 1);
    assert_eq!(e.text(), "e\u{301}", "a family is one grapheme");
    press(&mut e, &Change::DeleteBack, 1);
    assert_eq!(e.text(), "", "a letter and its accent are one grapheme");
    assert!(!e.change(&Change::DeleteBack), "nothing left to delete");
}

#[test]
fn a_word_move_skips_spaces_and_punctuation() {
    let layout = Clusters::default();
    let mut e = editing("hello, wide world");
    e.motion(Motion::WordBack, false, &layout);
    assert_eq!(e.caret(), 12);
    e.motion(Motion::WordBack, false, &layout);
    assert_eq!(e.caret(), 7);
    e.motion(Motion::TextStart, false, &layout);
    e.motion(Motion::WordForward, false, &layout);
    assert_eq!(e.caret(), 5);
    press(&mut e, &Change::DeleteWordForward, 1);
    assert_eq!(e.text(), "hello world");
}

#[test]
fn typed_letters_undo_together_and_a_space_ends_the_run() {
    let mut e = editing("");
    for c in ["a", "b", "c", " ", "d"] {
        e.change(&Change::Insert(c.into()));
    }
    assert_eq!(e.text(), "abc d");
    assert!(e.undo());
    assert_eq!(e.text(), "abc ");
    assert!(e.undo());
    assert_eq!(e.text(), "abc");
    assert!(e.undo());
    assert_eq!(e.text(), "");
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.text(), "abc");
}

#[test]
fn typing_over_a_selection_replaces_it_and_cut_takes_it() {
    let mut e = editing("hello world");
    e.place(0, false);
    e.place(5, true);
    assert_eq!(e.selected(), "hello");
    e.change(&Change::Insert("bye".into()));
    assert_eq!(e.text(), "bye world");
    assert_eq!(e.caret(), 3);
    e.select_all();
    assert_eq!(e.cut().as_deref(), Some("bye world"));
    assert_eq!(e.text(), "");
    assert_eq!(e.cut(), None);
}

#[test]
fn a_secret_shows_a_dot_for_each_grapheme() {
    let e = editing("e\u{301}😀");
    let shown = e.shown(true);
    assert_eq!(shown.text, "••");
    assert_eq!(shown.caret, "••".len());
    assert_eq!(e.shown(false).text, "e\u{301}😀");
}

#[test]
fn a_composition_shows_at_the_caret_until_it_commits() {
    let mut e = editing("ab");
    e.place(1, false);
    e.compose(Some("ㅎ"));
    let shown = e.shown(false);
    assert_eq!(shown.text, "aㅎb");
    assert_eq!(shown.composing, Some(1..1 + "ㅎ".len()));
    assert_eq!(shown.caret, 1 + "ㅎ".len());
    assert_eq!(e.text(), "ab", "a composition is not text yet");
    e.commit("한");
    assert_eq!(e.text(), "a한b");
    assert_eq!(e.shown(false).composing, None);
}

#[test]
fn one_line_turns_a_line_break_into_a_space() {
    assert_eq!(Editing::new("a\r\nb", false).text(), "a b");
    assert_eq!(Editing::new("a\r\nb", true).text(), "a\nb");
}

fn faces() -> Vec<crate::fonts::FontFace> {
    let mut faces = vec![crate::fonts::FontFace {
        name: "SourceSans3-Regular".into(),
        path: "fonts/SourceSans3-Regular.ttf".into(),
        system: false,
        bytes: Arc::new(include_bytes!("../../../editor/fonts/SourceSans3-Regular.ttf").to_vec()),
        tweak: crate::fonts::FaceTweak::default(),
    }];
    faces.extend(crate::fonts::system_faces());
    faces
}

fn layout(state: &mut TextState, text: &str, width: Option<f32>) -> Clusters {
    let mut request = Request::new(text, 20.0);
    request.width = width;
    state.clusters(&request.as_ref())
}

fn boundaries(text: &str) -> Vec<usize> {
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
        .collect()
}

/// Every caret stop, clicked where it is drawn, puts the caret back there.
fn round_trips(state: &mut TextState, text: &str) -> Clusters {
    let clusters = layout(state, text, None);
    for at in boundaries(text) {
        let (x, line) = clusters.caret(at);
        let l = clusters.lines[line];
        let back = clusters.offset_at(pos2(x, l.top + l.height / 2.0));
        let (bx, _) = clusters.caret(back);
        assert!(
            back == at || (bx - x).abs() < 0.01,
            "{text:?}: byte {at} is drawn at {x}, and a click there lands on byte {back}"
        );
    }
    clusters
}

#[test]
fn a_left_to_right_caret_walks_right_and_a_click_lands_back() {
    let mut state = TextState::new(&faces(), "en-US");
    let text = "Hello world";
    let clusters = round_trips(&mut state, text);
    let xs: Vec<f32> = boundaries(text)
        .iter()
        .map(|at| clusters.caret(*at).0)
        .collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
}

#[test]
fn a_right_to_left_caret_walks_left() {
    let mut state = TextState::new(&faces(), "en-US");
    if !state.covers('س') {
        eprintln!("skipped: no Arabic face on this machine");
        return;
    }
    let text = "سلام عليكم";
    let clusters = round_trips(&mut state, text);
    let xs: Vec<f32> = boundaries(text)
        .iter()
        .map(|at| clusters.caret(*at).0)
        .collect();
    assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
}

#[test]
fn hindi_thai_and_mixed_direction_text_round_trip() {
    let mut state = TextState::new(&faces(), "en-US");
    for (probe, text) in [('न', "नमस्ते दुनिया"), ('ส', "สวัสดีครับ"), ('ש', "abc שלום def")]
    {
        if !state.covers(probe) {
            eprintln!("skipped {text:?}: no face covers it on this machine");
            continue;
        }
        round_trips(&mut state, text);
    }
}

#[test]
fn a_wrapped_text_moves_down_a_line_and_home_and_end_keep_to_it() {
    let mut state = TextState::new(&faces(), "en-US");
    let text = "one two three four five six";
    let clusters = layout(&mut state, text, Some(80.0));
    assert!(clusters.lines.len() > 1, "the text wraps at 80 pixels");
    let below = clusters.vertical(0, true).expect("a line below the first");
    assert_eq!(clusters.caret(below).1, 1);
    let (start, end) = clusters.line_bytes(below);
    assert!(start > 0 && end <= text.len() && start < end);
    assert_eq!(
        clusters.vertical(0, false),
        None,
        "nothing above the first line"
    );
}

#[test]
fn an_empty_text_has_one_line_and_its_caret_at_the_start() {
    let mut state = TextState::new(&faces(), "en-US");
    let clusters = layout(&mut state, "", None);
    assert_eq!(clusters.lines.len(), 1);
    assert_eq!(clusters.caret(0), (0.0, 0));
    assert_eq!(clusters.offset_at(pos2(50.0, 5.0)), 0);
}
