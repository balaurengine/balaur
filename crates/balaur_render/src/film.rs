//! Where an authored colour meets the film.
//!
//! A colour a scene, a script or a material writes is the one a screen shows:
//! `#3c4753` is (60, 71, 83). The film holds linear light and its resolve
//! encodes it once, so each authored colour is decoded here on its way in.

/// The power the fork's resolve encodes the film with (`tonemap_ops.wgsl`).
/// Its exact inverse, so a colour comes back as the byte it was written as.
const ENCODE_POWER: f32 = 2.2;

/// One authored channel as linear light. Past one it keeps climbing, so a
/// colour written bright enough to bloom still is.
#[must_use]
pub fn linear(channel: f32) -> f32 {
    libm::powf(channel.max(0.0), ENCODE_POWER)
}

/// An authored colour as linear light.
#[must_use]
pub fn rgb([r, g, b]: [f32; 3]) -> [f32; 3] {
    [linear(r), linear(g), linear(b)]
}

/// An authored colour as linear light. Alpha is coverage, not light, and
/// passes as written.
#[must_use]
pub fn rgba([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [linear(r), linear(g), linear(b), a]
}

/// An authored colour as the one kiss3d draws into the film.
#[cfg(feature = "window")]
pub(crate) fn color(authored: [f32; 4]) -> kiss3d::color::Color {
    let [r, g, b, a] = rgba(authored);
    kiss3d::color::Color::new(r, g, b, a)
}

/// [`color`] for a colour with no alpha of its own.
#[cfg(feature = "window")]
pub(crate) fn opaque([r, g, b]: [f32; 3]) -> kiss3d::color::Color {
    color([r, g, b, 1.0])
}

#[cfg(test)]
mod tests {
    use super::{linear, rgba};

    /// The byte the fork's resolve writes for a linear channel, spelled as
    /// `apply_tonemap` spells it.
    fn resolved(channel: f32) -> u8 {
        (libm::powf(channel, 1.0 / 2.2) * 255.0).round() as u8
    }

    #[test]
    fn every_authored_byte_comes_back_from_the_film_as_written() {
        for written in 0..=255u8 {
            let authored = f32::from(written) / 255.0;
            assert_eq!(resolved(linear(authored)), written);
        }
    }

    #[test]
    fn an_authored_colour_is_darker_as_light_than_as_written() {
        let [r, g, b, a] = rgba([60.0 / 255.0, 71.0 / 255.0, 83.0 / 255.0, 0.5]);
        assert!(r < 0.05 && g < 0.07 && b < 0.09, "{r} {g} {b}");
        assert!((a - 0.5).abs() < f32::EPSILON, "alpha is not light");
    }

    #[test]
    fn a_channel_past_one_stays_past_one() {
        assert!(linear(2.0) > 1.0);
        assert!(linear(-1.0).abs() < f32::EPSILON);
    }
}
