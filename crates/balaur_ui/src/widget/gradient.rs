//! A theme table's gradient: which way it runs and its stops, and the colour
//! at any point along it. Read by `theme`, painted by `paint::gradient`.

use egui::Color32;

/// `gradient = { kind = "linear", stops = ["primary", "bg_panel"], angle = 90 }`,
/// or `kind = "radial"` with a `center` and a `radius`; `at` places the stops,
/// evenly spaced when left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub shape: GradientShape,
    /// Each stop's place, 0 to 1 and in order, and its colour.
    pub stops: Vec<(f32, Color32)>,
}

/// Which way a gradient runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientShape {
    /// Along a line through the box, `angle` degrees: 0 runs left to right,
    /// 90 top to bottom.
    Linear { angle: f32 },
    /// Out from `center`, a share of the box, to `radius`, a share of its
    /// longer side.
    Radial { center: [f32; 2], radius: f32 },
}

impl Gradient {
    /// The colour `t` of the way along, 0 to 1, between the stops either
    /// side of it.
    #[must_use]
    pub fn at(&self, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let Some(&(first_at, first)) = self.stops.first() else {
            return Color32::TRANSPARENT;
        };
        if t <= first_at {
            return first;
        }
        for pair in self.stops.windows(2) {
            let ((a, from), (b, to)) = (pair[0], pair[1]);
            if t <= b {
                let span = b - a;
                let share = if span > 0.0 { (t - a) / span } else { 1.0 };
                return from.lerp_to_gamma(to, share);
            }
        }
        self.stops.last().map_or(first, |(_, last)| *last)
    }
}
