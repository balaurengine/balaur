//! The fixed-step clock a play keeps: what `playback_time`, the queue and the
//! `finished` event are read from.

use balaur_audio::FileSettings;
use balaur_audio::playback::{Clip, Program, Timeline};

fn clip(looped: bool, loop_offset: f32) -> Clip {
    Clip::new(
        Vec::new(),
        FileSettings {
            looped,
            loop_offset,
            ..FileSettings::default()
        },
    )
}

#[test]
fn a_clock_waits_out_its_delay_then_counts_to_the_end() {
    let mut program = Program::new(clip(false, 0.0));
    program.delay = 0.5;
    let mut clock = Timeline::new(&program, &[Some(1.0)]);
    assert!(!clock.advance(0.25));
    assert!(clock.at.abs() < 1e-9, "the file holds during its delay");
    assert_eq!(clock.left(), Some(1.25));
    assert!(!clock.advance(0.5));
    assert!((clock.at - 0.25).abs() < 1e-9);
    assert!(clock.advance(0.75), "it ends with the file");
}

#[test]
fn start_and_end_time_bound_the_main_file() {
    let mut program = Program::new(clip(false, 0.0));
    program.start_time = 0.25;
    program.end_time = 0.75;
    let mut clock = Timeline::new(&program, &[Some(2.0)]);
    assert!((clock.at - 0.25).abs() < 1e-9);
    assert_eq!(clock.left(), Some(0.5));
    assert!(clock.advance(0.5));
}

#[test]
fn an_end_past_the_file_is_the_files_end() {
    let mut program = Program::new(clip(false, 0.0));
    program.end_time = 5.0;
    let clock = Timeline::new(&program, &[Some(1.0)]);
    assert_eq!(clock.left(), Some(1.0));
}

#[test]
fn a_loop_turns_back_at_its_offset_and_never_ends() {
    let mut program = Program::new(clip(false, 0.0));
    program.looped = true;
    program.loop_offset = Some(0.5);
    let mut clock = Timeline::new(&program, &[Some(1.0)]);
    assert!(!clock.advance(1.25));
    assert!((clock.at - 0.75).abs() < 1e-9, "{}", clock.at);
    assert!(!clock.advance(10.0));
    assert_eq!(clock.left(), None, "a loop has no end to fade towards");
    assert!(clock.counted(), "a loop is the clock's, not the sink's");
}

#[test]
fn the_queue_follows_the_main_file_and_each_file_starts_at_its_top() {
    let mut program = Program::new(clip(false, 0.0));
    program.start_time = 0.5;
    program.queue = vec![clip(false, 0.0), clip(false, 0.0)];
    let mut clock = Timeline::new(&program, &[Some(1.0), Some(1.0), Some(1.0)]);
    assert_eq!(clock.left(), Some(2.5));
    assert!(!clock.advance(0.75));
    assert_eq!(clock.index, 1);
    assert!(
        (clock.at - 0.25).abs() < 1e-9,
        "the queued file starts at 0"
    );
    assert!(clock.next_file());
    assert_eq!(clock.index, 2);
    assert!(!clock.next_file(), "there is no file past the last");
}

#[test]
fn a_queued_file_loops_by_its_own_import_setting() {
    let mut program = Program::new(clip(false, 0.0));
    program.queue = vec![clip(true, 0.25)];
    let mut clock = Timeline::new(&program, &[Some(1.0), Some(1.0)]);
    assert!(!clock.advance(2.5));
    assert_eq!(clock.index, 1);
    assert!((clock.at - 0.75).abs() < 1e-9, "{}", clock.at);
}

#[test]
fn a_seek_lands_inside_the_file_and_drops_the_delay() {
    let mut program = Program::new(clip(false, 0.0));
    program.delay = 1.0;
    let mut clock = Timeline::new(&program, &[Some(2.0)]);
    clock.seek(5.0);
    assert!(
        (clock.at - 2.0).abs() < 1e-9,
        "a seek past the end lands on it"
    );
    assert!(clock.delay.abs() < 1e-9);
    clock.seek(0.5);
    assert_eq!(clock.left(), Some(1.5));
    clock.seek(f64::NAN);
    assert!(
        clock.at.abs() < 1e-9,
        "a time that is not a number is the top"
    );
}

#[test]
fn a_file_of_unknown_length_is_left_to_its_sink() {
    let program = Program::new(clip(false, 0.0));
    let mut clock = Timeline::new(&program, &[None]);
    assert!(!clock.counted());
    assert!(
        !clock.advance(100.0),
        "the clock cannot end what it cannot measure"
    );
    assert_eq!(clock.left(), None);
}
