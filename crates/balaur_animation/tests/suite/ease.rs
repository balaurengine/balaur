//! The easing library, with no engine anywhere near it.
//!
//! A curve is a pure function of one number, so these assertions are about
//! arithmetic: that every name resolves, that the same input gives back the
//! same bits, and that the four modes stand in the relation their names
//! promise. Determinism is the reason the library exists — `f32::sin` and
//! `powf` route to the platform's libm and differ between operating systems —
//! so "the same bits" is the assertion that matters most here.

use balaur_animation::ease::{self, Easing, Mode, Transition};

/// Every curve, sampled at eleven points across its segment.
fn samples(curve: Easing) -> Vec<f32> {
    (0..=10).map(|i| curve.apply(i as f32 / 10.0)).collect()
}

#[test]
fn every_named_curve_is_reachable_by_name() {
    let names = ease::names();
    assert_eq!(
        names.len(),
        45,
        "eleven transitions in four modes, plus bare `linear`"
    );
    for name in names {
        let curve = Easing::parse(name).unwrap_or_else(|why| panic!("'{name}': {why:#}"));
        assert_eq!(curve.name(), name, "a curve forgot the name it was given");
    }
}

#[test]
fn a_curve_reads_its_own_name_back_as_a_transition_and_a_mode() {
    let curve = Easing::parse("in_out_elastic").unwrap();
    assert_eq!(curve.transition(), Transition::Elastic);
    assert_eq!(curve.mode(), Mode::InOut);
    let curve = Easing::parse("out_back").unwrap();
    assert_eq!(curve.transition(), Transition::Back);
    assert_eq!(curve.mode(), Mode::Out);
}

#[test]
fn a_curve_returns_exactly_the_same_bits_twice() {
    for name in ease::names() {
        let curve = Easing::parse(name).unwrap();
        let first: Vec<u32> = samples(curve).iter().map(|v| v.to_bits()).collect();
        let again: Vec<u32> = samples(curve).iter().map(|v| v.to_bits()).collect();
        assert_eq!(first, again, "'{name}' is not a function of its argument");
    }
}

/// Whether a curve landed exactly where it was sent.
///
/// A difference of exactly zero is the claim, not a tolerance. Written as a
/// subtraction rather than a bit comparison because `-0.0` is the same point
/// as `0.0` — `in_back`'s shape leaves a signed zero at `u = 0`, and a tween
/// told to land on zero that lands on `-0.0` has landed.
fn lands_exactly_on(got: f32, want: f32) -> bool {
    got - want == 0.0
}

#[test]
fn every_curve_starts_at_zero_and_ends_at_one() {
    // Exactly, not nearly: a tween that lands a hair short of its `to` would
    // leave a node one float off wherever it was sent, every time.
    for name in ease::names() {
        let curve = Easing::parse(name).unwrap();
        assert!(
            lands_exactly_on(curve.apply(0.0), 0.0),
            "'{name}' does not start where it is"
        );
        assert!(
            lands_exactly_on(curve.apply(1.0), 1.0),
            "'{name}' does not arrive"
        );
    }
}

#[test]
fn a_curve_name_nothing_defines_is_rejected_naming_it() {
    let why = Easing::parse("out_wobble").unwrap_err().to_string();
    assert!(why.contains("out_wobble"), "{why}");
    assert!(why.contains("in_out"), "the message lists the modes: {why}");
    assert!(why.contains("elastic"), "and the transitions: {why}");
}

#[test]
fn an_out_curve_is_its_in_curve_reflected() {
    for transition in ["sine", "quad", "cubic", "expo", "circ", "back", "bounce"] {
        let rising = Easing::parse(&format!("in_{transition}")).unwrap();
        let falling = Easing::parse(&format!("out_{transition}")).unwrap();
        for i in 0..=10 {
            let u = i as f32 / 10.0;
            let reflected = 1.0 - rising.apply(1.0 - u);
            assert!(
                (falling.apply(u) - reflected).abs() < 1e-6,
                "out_{transition}({u}) is {} but in_{transition} reflected is {reflected}",
                falling.apply(u)
            );
        }
    }
}

#[test]
fn an_in_out_curve_meets_in_the_middle() {
    for name in ease::names() {
        let curve = Easing::parse(name).unwrap();
        if curve.mode() != Mode::InOut && curve.mode() != Mode::OutIn {
            continue;
        }
        assert!(
            (curve.apply(0.5) - 0.5).abs() < 1e-6,
            "'{name}' hands over at {} rather than halfway",
            curve.apply(0.5)
        );
    }
}

#[test]
fn linear_is_one_straight_line_with_no_modes() {
    let curve = Easing::parse("linear").unwrap();
    for i in 0..=10 {
        let u = i as f32 / 10.0;
        assert!(
            (curve.apply(u) - u).abs() < 1e-6,
            "linear bends: {u} became {}",
            curve.apply(u)
        );
    }
    for moded in ["in_linear", "out_linear", "in_out_linear", "out_in_linear"] {
        assert!(Easing::parse(moded).is_err(), "'{moded}' parses");
    }
}

#[test]
fn a_curve_matches_the_shape_godot_gives_that_name() {
    // Pinned so that "ported from Godot behaves the same" is a fact rather
    // than an intention. Every number is the closed form of Godot's own
    // easing equation at that point.
    let cases = [
        ("in_quad", 0.5, 0.25),
        ("out_quad", 0.5, 0.75),
        ("in_out_quad", 0.25, 0.125),
        ("in_cubic", 0.5, 0.125),
        ("in_back", 0.5, -0.087_697_5),
        ("out_back", 0.5, 1.087_697_5),
        ("out_bounce", 0.5, 0.765_625),
        ("in_expo", 0.5, 0.03125),
        ("in_circ", 0.5, 0.133_974_6),
    ];
    for (name, at, want) in cases {
        let got = Easing::parse(name).unwrap().apply(at);
        assert!(
            (got - want).abs() < 1e-5,
            "'{name}' at {at} is {got}, not Godot's {want}"
        );
    }
}

#[test]
fn back_and_elastic_overshoot_where_quad_does_not() {
    let quad = Easing::parse("out_quad").unwrap();
    let back = Easing::parse("out_back").unwrap();
    let elastic = Easing::parse("out_elastic").unwrap();
    let over = |curve: Easing| (0..=100).any(|i| curve.apply(i as f32 / 100.0) > 1.0);
    assert!(!over(quad), "out_quad has no business overshooting");
    assert!(over(back), "out_back is supposed to overshoot");
    assert!(over(elastic), "out_elastic is supposed to overshoot");
}

// CSS's curves at five points, against the curve solved to the last bit:
// `ease`, `ease-in`, `ease-out`, `ease-in-out`, and the two a site moves by.
#[test]
fn a_css_curve_moves_as_a_browser_moves_it() {
    let at: [f32; 5] = [0.1, 0.25, 0.5, 0.75, 0.9];
    for (curve, want) in [
        (
            "cubic-bezier(0.25, 0.1, 0.25, 1)",
            [
                0.094_796_306,
                0.408_510_591,
                0.802_403_388,
                0.960_458_978,
                0.994_316_477,
            ],
        ),
        (
            "cubic-bezier(0.42, 0, 1, 1)",
            [
                0.017_026_61,
                0.093_464_651,
                0.315_356_813,
                0.621_861_869,
                0.839_427_846,
            ],
        ),
        (
            "cubic-bezier(0, 0, 0.58, 1)",
            [
                0.160_572_154,
                0.378_138_131,
                0.684_643_187,
                0.906_535_349,
                0.982_973_39,
            ],
        ),
        (
            "cubic-bezier(0.42, 0, 0.58, 1)",
            [
                0.019_722_454,
                0.129_161_931,
                0.5,
                0.870_838_069,
                0.980_277_546,
            ],
        ),
        (
            "cubic-bezier(0.2, 0.7, 0.2, 1)",
            [
                0.359_122_765,
                0.727_373_396,
                0.929_578_821,
                0.987_468_037,
                0.998_286_622,
            ],
        ),
        (
            "cubic-bezier(0.3, 1.4, 0.5, 1)",
            [
                0.414_424_326,
                0.826_193_523,
                1.048_828_113,
                1.027_279_461,
                1.005_054_134,
            ],
        ),
    ] {
        let eased = Easing::parse(curve).unwrap();
        for (u, want) in at.iter().zip(want) {
            let got = f64::from(eased.apply(*u));
            assert!(
                (got - want).abs() < 1e-6,
                "{curve} at {u}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn a_css_curve_lands_on_its_ends_and_runs_on_straight_past_them() {
    let ease = Easing::parse("cubic-bezier(0.25, 0.1, 0.25, 1)").unwrap();
    assert!(lands_exactly_on(ease.apply(0.0), 0.0));
    assert!(lands_exactly_on(ease.apply(1.0), 1.0));
    // It leaves along y1 / x1 = 0.4 and arrives flat.
    assert!(
        (ease.apply(-0.5) + 0.2).abs() < 1e-6,
        "{}",
        ease.apply(-0.5)
    );
    assert!(lands_exactly_on(ease.apply(1.5), 1.0));
    let straight = Easing::parse("cubic-bezier(0.3, 0.3, 0.6, 0.6)").unwrap();
    assert!(lands_exactly_on(straight.apply(0.37), 0.37));
}

#[test]
fn a_css_curve_keeps_the_name_css_writes_it_with() {
    let curve = Easing::parse(" Cubic-Bezier(.2,0.7,  0.2 , 1.0)").unwrap();
    assert_eq!(curve.name(), "cubic-bezier(0.2, 0.7, 0.2, 1)");
    assert_eq!(Easing::parse(&curve.name()).unwrap(), curve);
    assert_eq!(curve.transition(), Transition::CubicBezier);
    assert_eq!(curve.mode(), Mode::In);
}

#[test]
fn a_css_curve_css_would_refuse_is_refused() {
    for bad in [
        "cubic-bezier(1.2, 0, 0.5, 1)",
        "cubic-bezier(0.2, 0, -0.1, 1)",
        "cubic-bezier(0.2, 0, 1)",
        "cubic-bezier(0.2, 0, 0.5, 1, 1)",
        "cubic-bezier(a, 0, 0.5, 1)",
        "cubic-bezier(0.2, 0, 0.5, 1",
    ] {
        assert!(Easing::parse(bad).is_err(), "{bad}");
    }
}
