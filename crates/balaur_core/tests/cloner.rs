//! Where a cloner's copies land: the three modes, the scatter, and the cap
//! that stops a typo laying out a million of them.

use balaur_core::cloner::{Cloner, MAX_CLONES, Mode};
use glamx::Vec3;

fn linear(count: u32, step: Vec3) -> Cloner {
    Cloner {
        mode: Mode::Linear,
        count,
        step,
        ..Cloner::default()
    }
}

#[test]
fn a_linear_cloner_walks_the_step() {
    let copies = linear(4, Vec3::new(2.0, 0.0, 0.0)).clones();
    assert_eq!(copies.len(), 4);
    assert_eq!(copies[0].position, Vec3::ZERO, "the first is the template");
    assert_eq!(copies[3].position, Vec3::new(6.0, 0.0, 0.0));
    assert!(copies.iter().all(|c| c.scale == Vec3::ONE));
}

#[test]
fn a_grid_cloner_fills_a_box() {
    let cloner = Cloner {
        mode: Mode::Grid,
        counts: [5, 2, 3],
        step: Vec3::new(1.0, 2.0, 3.0),
        ..Cloner::default()
    };
    let copies = cloner.clones();
    assert_eq!(copies.len(), 30);
    let widest = copies
        .iter()
        .fold(Vec3::ZERO, |widest, c| widest.max(c.position));
    assert_eq!(widest, Vec3::new(4.0, 2.0, 6.0), "the far corner");
}

/// A ring with no angle closes on itself, and each copy faces the way the
/// ring goes.
#[test]
fn a_radial_cloner_closes_its_ring() {
    let cloner = Cloner {
        mode: Mode::Radial,
        count: 8,
        radius: 3.0,
        ..Cloner::default()
    };
    let copies = cloner.clones();
    assert_eq!(copies.len(), 8);
    for clone in &copies {
        let reach = Vec3::new(clone.position.x, 0.0, clone.position.z).length();
        assert!(
            (reach - 3.0).abs() < 1e-4,
            "a copy sat {reach} from the axis"
        );
    }
    assert!(
        (copies[0].position - Vec3::new(3.0, 0.0, 0.0)).length() < 1e-5,
        "the first copy starts on x"
    );
    assert_ne!(
        copies[1].rotation, copies[0].rotation,
        "each faces outwards"
    );
}

#[test]
fn an_angle_overrides_the_closing_ring() {
    let cloner = Cloner {
        mode: Mode::Radial,
        count: 3,
        radius: 1.0,
        angle: 10.0,
        ..Cloner::default()
    };
    let copies = cloner.clones();
    let last = copies[2].position;
    assert!(last.x > 0.9, "twenty degrees round is still nearly on x");
    assert!(last.z > 0.0 && last.z < 0.5);
}

/// Nothing wanders without a seed, however large the range.
#[test]
fn a_cloner_with_no_seed_lays_out_exactly() {
    let cloner = Cloner {
        random: 1.0,
        ..linear(5, Vec3::X)
    };
    assert_eq!(cloner.clones(), linear(5, Vec3::X).clones());
}

#[test]
fn a_seed_scatters_and_the_same_seed_scatters_the_same_way() {
    let scattered = Cloner {
        seed: 7,
        random: 0.5,
        ..linear(6, Vec3::X)
    };
    let copies = scattered.clones();
    let plain = linear(6, Vec3::X).clones();
    assert_ne!(copies, plain, "a seed should have moved something");
    assert_eq!(
        copies,
        scattered.clones(),
        "and moved it the same way twice"
    );
    let other = Cloner {
        seed: 8,
        ..scattered
    };
    assert_ne!(
        copies,
        other.clones(),
        "a different seed lays out differently"
    );
    for (clone, cell) in copies.iter().zip(&plain) {
        let drift = (clone.position - cell.position).length();
        assert!(drift <= 0.9, "a copy wandered {drift}, more than the range");
        assert!(
            clone.scale.x > 0.4 && clone.scale.x < 1.6,
            "scale stayed sane"
        );
    }
}

/// A grid whose counts multiply out past the cap stops at it rather than
/// filling memory.
#[test]
fn a_cloner_stops_at_the_cap() {
    let cloner = Cloner {
        mode: Mode::Grid,
        counts: [500, 500, 500],
        ..Cloner::default()
    };
    assert_eq!(cloner.clones().len(), MAX_CLONES);
}

#[test]
fn a_count_of_none_still_draws_the_template_once() {
    assert_eq!(linear(0, Vec3::X).clones().len(), 1);
}

#[test]
fn every_mode_has_a_word_and_answers_to_it() {
    for mode in [Mode::Linear, Mode::Radial, Mode::Grid] {
        assert_eq!(Mode::from_word(mode.word()), Some(mode));
    }
    assert_eq!(Mode::from_word("sprinkle"), None);
}
