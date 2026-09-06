//! Where a cloner's copies go: a pure function from its parameters to a list
//! of poses.
//!
//! Nothing about drawing is here, which is what lets a test count copies and
//! a bake write them out as real nodes without a window. The scatter runs off
//! the seed alone, so the same cloner lays out the same way on every machine
//! and in a replay.

use glamx::{Quat, Vec3};

/// How the copies are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// A run of copies, each one `step` further on.
    Linear,
    /// A ring of copies, `angle` degrees apart at `radius`.
    Radial,
    /// A box of copies, `counts` of them along each axis.
    Grid,
}

impl Mode {
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Linear => words::LINEAR,
            Self::Radial => words::RADIAL,
            Self::Grid => words::GRID,
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            words::LINEAR => Some(Self::Linear),
            words::RADIAL => Some(Self::Radial),
            words::GRID => Some(Self::Grid),
            _ => None,
        }
    }
}

/// The modes, spelled once for a schema, a scene and a script.
pub mod words {
    pub const LINEAR: &str = "linear";
    pub const RADIAL: &str = "radial";
    pub const GRID: &str = "grid";
    /// In the order an inspector offers them.
    pub const MODES: &[&str] = &[LINEAR, RADIAL, GRID];
}

/// Every key a cloner reads.
pub mod keys {
    pub const MODE: &str = "mode";
    pub const COUNT: &str = "count";
    pub const COUNTS: &str = "counts";
    pub const STEP: &str = "step";
    pub const RADIUS: &str = "radius";
    pub const ANGLE: &str = "angle";
    pub const SEED: &str = "seed";
    pub const RANDOM: &str = "random";
}

/// The most copies one cloner will lay out. A grid of three counts multiplies
/// out fast, and a typo should cost a frame rather than the process.
pub const MAX_CLONES: usize = 16_384;

/// A cloner's parameters, as a scene writes them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cloner {
    pub mode: Mode,
    /// How many, for linear and radial.
    pub count: u32,
    /// How many along each axis, for a grid.
    pub counts: [u32; 3],
    /// The gap between copies, for linear and grid.
    pub step: Vec3,
    pub radius: f32,
    /// Degrees between copies on a ring; a full turn divided by `count` when
    /// it is zero, which is what a ring usually wants.
    pub angle: f32,
    /// Zero scatters nothing, whatever `random` says.
    pub seed: u64,
    /// How far a copy may wander, as a fraction: of the step in position, of
    /// a half turn in rotation, and of its own size in scale.
    pub random: f32,
}

impl Default for Cloner {
    fn default() -> Self {
        Self {
            mode: Mode::Linear,
            count: 4,
            counts: [3, 1, 3],
            step: Vec3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            angle: 0.0,
            seed: 0,
            random: 0.0,
        }
    }
}

/// One copy: where it sits relative to the cloner's own node.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Clone3d {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// A stream of numbers from a seed, the same on every platform.
///
/// `splitmix64`: one multiply-shift round per number, no state to carry
/// between frames, and a cloner that is asked twice answers twice the same.
struct Scatter(u64);

impl Scatter {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        // The top 24 bits as a fraction: every value exact in f32.
        (z >> 40) as f32 / f32::from(1u16 << 8).mul_add(65536.0, 0.0)
    }

    /// A number from -1 to 1.
    fn signed(&mut self) -> f32 {
        self.next().mul_add(2.0, -1.0)
    }
}

impl Cloner {
    /// Where every copy goes, in the cloner node's own space. The first is
    /// always at the origin, so a cloner with one copy draws its template
    /// exactly where the template already is.
    #[must_use]
    pub fn clones(&self) -> Vec<Clone3d> {
        let mut out = match self.mode {
            Mode::Linear => self.linear(),
            Mode::Radial => self.radial(),
            Mode::Grid => self.grid(),
        };
        if self.seed != 0 && self.random > 0.0 {
            self.scatter(&mut out);
        }
        out
    }

    fn linear(&self) -> Vec<Clone3d> {
        let count = (self.count.max(1) as usize).min(MAX_CLONES);
        (0..count)
            .map(|i| Clone3d {
                position: self.step * i as f32,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            })
            .collect()
    }

    fn radial(&self) -> Vec<Clone3d> {
        let count = (self.count.max(1) as usize).min(MAX_CLONES);
        // A ring with no angle given closes on itself, which is what a ring
        // of anything is nearly always meant to be.
        let step = if self.angle == 0.0 {
            360.0 / count as f32
        } else {
            self.angle
        };
        (0..count)
            .map(|i| {
                let turn = (step * i as f32).to_radians();
                let (sin, cos) = libm::sincosf(turn);
                Clone3d {
                    position: Vec3::new(self.radius * cos, 0.0, self.radius * sin),
                    // Facing outwards, so a ring of fence posts leans the
                    // way the ring goes rather than all one way.
                    rotation: Quat::from_rotation_y(-turn),
                    scale: Vec3::ONE,
                }
            })
            .collect()
    }

    fn grid(&self) -> Vec<Clone3d> {
        let counts = self.counts.map(|n| n.max(1) as usize);
        let total = counts[0] * counts[1] * counts[2];
        let mut out = Vec::with_capacity(total.min(MAX_CLONES));
        for x in 0..counts[0] {
            for y in 0..counts[1] {
                for z in 0..counts[2] {
                    if out.len() >= MAX_CLONES {
                        return out;
                    }
                    let cell = Vec3::new(x as f32, y as f32, z as f32);
                    out.push(Clone3d {
                        position: self.step * cell,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::ONE,
                    });
                }
            }
        }
        out
    }

    /// Wander each copy off its cell, the first one included: a scattered
    /// cloner with the seed changed lays out differently everywhere.
    fn scatter(&self, clones: &mut [Clone3d]) {
        let mut noise = Scatter(self.seed);
        let reach = self.random.clamp(0.0, 1.0);
        for clone in clones.iter_mut() {
            let drift = Vec3::new(noise.signed(), noise.signed(), noise.signed());
            clone.position += drift * self.step * reach;
            let turn = noise.signed() * reach * std::f32::consts::PI;
            clone.rotation *= Quat::from_rotation_y(turn);
            let grow = noise.signed().mul_add(reach * 0.5, 1.0).max(0.05);
            clone.scale *= grow;
        }
    }
}
