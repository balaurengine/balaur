//! The deterministic stream is an engine service, so it must be the same
//! sequence whatever language asks for it. A replay that diverges by one
//! random number diverges completely.

use balaur_core::rng::Pcg32;

/// Pinned values, not a self-comparison: if the generator changes, every
/// recorded replay breaks, so that has to be a deliberate act.
#[test]
fn the_stream_is_pinned_for_a_given_seed() {
    let mut rng = Pcg32::new(42);
    assert_eq!(rng.next_range_i64(1, 6), 5);
    assert!((rng.next_f64() - 0.418_087_280_513_577_13).abs() < f64::EPSILON);

    let mut again = Pcg32::new(42);
    assert_eq!(again.next_range_i64(1, 6), 5, "same seed, same first draw");
}

#[test]
fn a_different_seed_gives_a_different_stream() {
    let first: Vec<f64> = (0..4)
        .map({
            let mut r = Pcg32::new(1);
            move |_| r.next_f64()
        })
        .collect();
    let second: Vec<f64> = (0..4)
        .map({
            let mut r = Pcg32::new(2);
            move |_| r.next_f64()
        })
        .collect();
    assert_ne!(first, second);
}

#[test]
fn an_empty_or_inverted_range_does_not_panic() {
    let mut rng = Pcg32::new(7);
    assert_eq!(rng.next_range_i64(3, 3), 3);
    assert_eq!(rng.next_range_i64(9, 2), 9, "inverted range clamps to lo");
}
