//! An image file's pixels, read the one way every reader reads them: the
//! renderer, the UI, a headless size check and the exporter.
//!
//! A raster is decoded; an SVG is rasterized at its `scale`; then the texel
//! settings of [`crate::import::texture::Texels`] are applied. An exported
//! pack holds the SVG's raster under the SVG's own name, so a game template
//! built without the `svg` feature never needs the rasterizer.

use anyhow::{Result, anyhow};
use image::RgbaImage;

use crate::import::texture::{self, Texels};

/// How the caller blends what it uploads. Premultiplied alpha zeroes a
/// transparent texel's colour, which leaves nothing for `bleed` to fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alpha {
    Straight,
    Premultiplied,
}

/// The largest side a rasterized SVG is given, whatever its `scale` asks.
pub const MAX_SIDE: u32 = 16_384;

/// Whether these bytes are an SVG document rather than a raster. No raster
/// format starts with `<`, and an SVG names its root within its preamble.
#[must_use]
pub fn is_svg(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let Some(start) = text.iter().position(|b| !b.is_ascii_whitespace()) else {
        return false;
    };
    let head = &text[start..text.len().min(start + 1024)];
    head.first() == Some(&b'<') && head.windows(4).any(|w| w == b"<svg")
}

/// The pixel size these bytes read as under `settings`: the header of a
/// raster, or an SVG's own size times its `scale`.
///
/// # Errors
/// If the bytes are no image this build reads.
pub fn size(bytes: &[u8], settings: &toml::Table) -> Result<(u32, u32)> {
    if is_svg(bytes) {
        return svg::size(bytes, texture::texels(settings).scale);
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_dimensions()
        .map_err(|why| anyhow!("{why}"))
}

/// The pixels, ready to upload: decoded or rasterized, then prepared by the
/// file's texel settings.
///
/// # Errors
/// If the bytes are no image this build reads.
pub fn decode(bytes: &[u8], settings: &toml::Table, alpha: Alpha) -> Result<RgbaImage> {
    let texels = texture::texels(settings);
    let mut image = if is_svg(bytes) {
        svg::rasterize(bytes, texels.scale)?
    } else {
        image::load_from_memory(bytes)?.to_rgba8()
    };
    prepare(&mut image, &texels, alpha);
    Ok(image)
}

/// Apply the texel settings to pixels already decoded.
pub fn prepare(image: &mut RgbaImage, texels: &Texels, alpha: Alpha) {
    if texels.flip_green {
        flip_green(image);
    }
    if texels.bleed && alpha == Alpha::Straight {
        bleed(image);
    }
}

/// Invert every texel's green, which turns a DirectX normal map into the
/// convention the renderer samples.
pub fn flip_green(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        pixel[1] = 255 - pixel[1];
    }
}

/// Give every fully transparent texel the average colour of its nearest
/// visible neighbours, ring by ring outward, leaving its alpha at zero.
///
/// A linear filter at a sprite's edge reads half a transparent texel; this is
/// what stops that half being the black or white an editor saved there.
pub fn bleed(image: &mut RgbaImage) {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let mut filled: Vec<bool> = image.pixels().map(|p| p[3] > 0).collect();
    if !filled.contains(&false) || !filled.contains(&true) {
        return;
    }
    let mut queued = vec![false; width * height];
    let mut ring = Vec::new();
    for at in 0..width * height {
        if filled[at] {
            continue;
        }
        let (around, count) = neighbours(at, width, height);
        if around[..count].iter().any(|n| filled[*n]) {
            queued[at] = true;
            ring.push(at);
        }
    }
    let data: &mut [u8] = image;
    let mut colours: Vec<[u8; 3]> = Vec::new();
    while !ring.is_empty() {
        colours.clear();
        for &at in &ring {
            let (around, count) = neighbours(at, width, height);
            let (mut sum, mut seen) = ([0u32; 3], 0u32);
            for &n in &around[..count] {
                if filled[n] {
                    sum[0] += u32::from(data[n * 4]);
                    sum[1] += u32::from(data[n * 4 + 1]);
                    sum[2] += u32::from(data[n * 4 + 2]);
                    seen += 1;
                }
            }
            let seen = seen.max(1);
            colours.push([
                (sum[0] / seen) as u8,
                (sum[1] / seen) as u8,
                (sum[2] / seen) as u8,
            ]);
        }
        for (&at, colour) in ring.iter().zip(&colours) {
            data[at * 4..at * 4 + 3].copy_from_slice(colour);
            filled[at] = true;
        }
        let mut next = Vec::new();
        for &at in &ring {
            let (around, count) = neighbours(at, width, height);
            for &n in &around[..count] {
                if !filled[n] && !queued[n] {
                    queued[n] = true;
                    next.push(n);
                }
            }
        }
        ring = next;
    }
}

/// The up to eight texels around `at` on a grid `width` wide, and how many
/// there are: plain loops, since this runs per texel in an unoptimised build.
fn neighbours(at: usize, width: usize, height: usize) -> ([usize; 8], usize) {
    let (x, y) = (at % width, at / width);
    let (mut out, mut count) = ([0usize; 8], 0);
    for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
        for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
            if (nx, ny) != (x, y) {
                out[count] = ny * width + nx;
                count += 1;
            }
        }
    }
    (out, count)
}

#[cfg(feature = "svg")]
mod svg {
    use anyhow::{Result, anyhow};
    use image::RgbaImage;
    use resvg::{tiny_skia, usvg};

    fn tree(bytes: &[u8]) -> Result<usvg::Tree> {
        usvg::Tree::from_data(bytes, &usvg::Options::default())
            .map_err(|why| anyhow!("reading the SVG: {why}"))
    }

    fn pixels(tree: &usvg::Tree, scale: f32) -> (u32, u32) {
        let side = |units: f32| ((units * scale).round() as u32).clamp(1, super::MAX_SIDE);
        (side(tree.size().width()), side(tree.size().height()))
    }

    pub(super) fn size(bytes: &[u8], scale: f32) -> Result<(u32, u32)> {
        Ok(pixels(&tree(bytes)?, scale))
    }

    /// Straight alpha out, as every decoded raster is: tiny-skia draws
    /// premultiplied.
    pub(super) fn rasterize(bytes: &[u8], scale: f32) -> Result<RgbaImage> {
        let tree = tree(bytes)?;
        let (width, height) = pixels(&tree, scale);
        let mut canvas = tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| anyhow!("an SVG of {width}x{height} pixels"))?;
        let fit = tiny_skia::Transform::from_scale(
            width as f32 / tree.size().width(),
            height as f32 / tree.size().height(),
        );
        resvg::render(&tree, fit, &mut canvas.as_mut());
        let straight: Vec<u8> = canvas
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        RgbaImage::from_raw(width, height, straight).ok_or_else(|| anyhow!("the SVG's pixels"))
    }
}

#[cfg(not(feature = "svg"))]
mod svg {
    use anyhow::{Result, bail};
    use image::RgbaImage;

    const REFUSED: &str = "this build reads no SVG; `balaur export` rasterizes one into the pack";

    pub(super) fn size(_: &[u8], _: f32) -> Result<(u32, u32)> {
        bail!(REFUSED)
    }

    pub(super) fn rasterize(_: &[u8], _: f32) -> Result<RgbaImage> {
        bail!(REFUSED)
    }
}

/// An SVG rasterized at `scale`, straight alpha. The exporter's way in.
///
/// # Errors
/// If the document does not parse, or the build has no rasterizer.
pub fn rasterize_svg(bytes: &[u8], scale: f32) -> Result<RgbaImage> {
    svg::rasterize(bytes, scale)
}

#[cfg(test)]
mod tests {
    use super::{Alpha, bleed, decode, flip_green, is_svg};
    use image::{Rgba, RgbaImage};

    #[test]
    fn an_svg_is_told_from_a_raster_by_its_root() {
        assert!(is_svg(b"<svg xmlns='http://www.w3.org/2000/svg'/>"));
        assert!(is_svg(b"\xEF\xBB\xBF  <?xml version='1.0'?>\n<svg/>"));
        assert!(!is_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg(b"<html></html>"));
    }

    /// A transparent texel takes its visible neighbour's colour and stays
    /// transparent; the visible one is untouched.
    #[test]
    fn bleeding_colours_the_transparent_texels_and_keeps_their_alpha() {
        let mut image = RgbaImage::from_pixel(4, 1, Rgba([0, 0, 0, 0]));
        image.put_pixel(0, 0, Rgba([200, 100, 50, 255]));
        bleed(&mut image);
        assert_eq!(image.get_pixel(0, 0), &Rgba([200, 100, 50, 255]));
        assert_eq!(image.get_pixel(3, 0), &Rgba([200, 100, 50, 0]));
    }

    #[test]
    fn an_opaque_or_empty_image_is_left_alone() {
        let mut empty = RgbaImage::from_pixel(2, 2, Rgba([9, 9, 9, 0]));
        bleed(&mut empty);
        assert_eq!(empty.get_pixel(1, 1), &Rgba([9, 9, 9, 0]));
    }

    #[test]
    fn flipping_green_inverts_it_alone() {
        let mut image = RgbaImage::from_pixel(1, 1, Rgba([10, 20, 30, 40]));
        flip_green(&mut image);
        assert_eq!(image.get_pixel(0, 0), &Rgba([10, 235, 30, 40]));
    }

    /// A premultiplied upload zeroes a transparent texel's colour anyway, so
    /// bleeding is skipped for it.
    #[test]
    fn a_premultiplied_upload_is_not_bled() {
        let mut png = Vec::new();
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 0]));
        image.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let none = toml::Table::new();
        assert_eq!(
            decode(&png, &none, Alpha::Straight)
                .unwrap()
                .get_pixel(1, 0)[0],
            255
        );
        assert_eq!(
            decode(&png, &none, Alpha::Premultiplied)
                .unwrap()
                .get_pixel(1, 0)[0],
            0
        );
    }

    #[cfg(feature = "svg")]
    #[test]
    fn an_svg_is_rasterized_at_its_scale() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="4"><rect width="10" height="4" fill="#ff0000"/></svg>"##;
        let settings: toml::Table = toml::from_str("scale = 2").unwrap();
        assert_eq!(super::size(svg, &settings).unwrap(), (20, 8));
        let image = decode(svg, &settings, Alpha::Straight).unwrap();
        assert_eq!(image.dimensions(), (20, 8));
        assert_eq!(image.get_pixel(5, 3), &Rgba([255, 0, 0, 255]));
    }
}
