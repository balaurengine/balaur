//! Images on their way into a pack: an SVG rasterized at its `scale`, and
//! anything past `[export] max_size` scaled down, with the size it was drawn
//! at recorded beside it so a sprite over it keeps its extent.
//!
//! Runs before `recode`, which may re-encode the result. A pack key never
//! changes: `art/logo.svg` holds a PNG, and every reader goes by content.

use std::collections::BTreeSet;
use std::io::Cursor;

use anyhow::{Context, Result, anyhow};
use balaur::import::{keys, texture, word, words};
use image::{DynamicImage, ImageFormat};

/// JPEG's quality when a capped photo is written again.
const JPEG_QUALITY: u8 = 90;

/// What one image ships as.
#[derive(Debug)]
pub(crate) struct Shipped {
    pub bytes: Vec<u8>,
    /// The size to record as `size`, when the shipped copy is smaller than
    /// what the scene measures.
    pub drawn: Option<(u32, u32)>,
}

/// One image's bytes as a target ships them, or `None` to keep them.
///
/// An SVG always becomes a raster. A raster is capped at `max_size` pixels on
/// its longer side, except pixel art, a bitmap font's page and a file whose
/// `recode` is `original`: each of those has a pixel something counts.
pub(crate) fn ship(
    bytes: &[u8],
    settings: &toml::Table,
    max_size: u32,
    font_page: bool,
) -> Result<Option<Shipped>> {
    let kept = word(settings, keys::RECODE, "") == words::ORIGINAL;
    let capped = max_size > 0 && !kept && !font_page && !texture::is_pixel_art(settings);
    let cap = if capped { max_size } else { 0 };
    if balaur::pixels::is_svg(bytes) {
        return rasterize(bytes, settings, cap).map(Some);
    }
    if cap == 0 {
        return Ok(None);
    }
    let format = image::guess_format(bytes).context("reading the image")?;
    let full = balaur::pixels::size(bytes, settings)?;
    let fit = fit(full, cap);
    if fit >= 1.0 {
        return Ok(None);
    }
    let (width, height) = scaled(full, fit);
    let smaller = image::load_from_memory_with_format(bytes, format)?.resize_exact(
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    Ok(Some(Shipped {
        bytes: encode(&smaller, format)?,
        drawn: Some(full),
    }))
}

/// An SVG's raster, drawn straight at the capped size rather than scaled
/// down after: the curves are sharper that way.
fn rasterize(bytes: &[u8], settings: &toml::Table, cap: u32) -> Result<Shipped> {
    let full = balaur::pixels::size(bytes, settings)?;
    let fit = if cap > 0 {
        fit(full, cap).min(1.0)
    } else {
        1.0
    };
    let scale = texture::texels(settings).scale * fit;
    let raster = balaur::pixels::rasterize_svg(bytes, scale)?;
    let drawn = (raster.dimensions() != full).then_some(full);
    Ok(Shipped {
        bytes: encode(&DynamicImage::ImageRgba8(raster), ImageFormat::Png)?,
        drawn,
    })
}

/// The fraction of `size` that fits inside `cap` on its longer side.
fn fit(size: (u32, u32), cap: u32) -> f32 {
    cap as f32 / size.0.max(size.1).max(1) as f32
}

fn scaled(size: (u32, u32), fraction: f32) -> (u32, u32) {
    let shrink = |n: u32| ((n as f32 * fraction).round() as u32).max(1);
    (shrink(size.0), shrink(size.1))
}

/// Pixels written in the format they came in, so a later re-encode chooses
/// from the same place it would have. WebP is written lossless.
fn encode(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if format == ImageFormat::Jpeg {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
        image.to_rgb8().write_with_encoder(encoder)?;
        return Ok(out);
    }
    let format = if format == ImageFormat::WebP {
        ImageFormat::WebP
    } else {
        ImageFormat::Png
    };
    image
        .write_to(&mut Cursor::new(&mut out), format)
        .map_err(|why| anyhow!("encoding the image: {why}"))?;
    Ok(out)
}

/// Every image a bitmap font draws its glyphs from, by pack key: the page
/// sits beside its `.fnt`, as the tool that wrote it left it.
pub(crate) fn font_pages<'a>(fonts: impl Iterator<Item = (&'a str, &'a [u8])>) -> BTreeSet<String> {
    let mut pages = BTreeSet::new();
    for (path, bytes) in fonts {
        if !path.to_ascii_lowercase().ends_with(".fnt") {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        pages.extend(balaur_text::bitmap::page_of(path, text));
    }
    pages
}

/// Write `drawn` into a sidecar as `size`, unless one is recorded already: a
/// folded variant's is the original's, and a cap of it must not overwrite it.
pub(crate) fn record_drawn(sidecar: Option<&str>, drawn: (u32, u32)) -> Option<String> {
    let mut settings: toml::Table = sidecar
        .and_then(|text| toml::from_str(text).ok())
        .unwrap_or_default();
    if settings.contains_key(keys::SIZE) {
        return None;
    }
    settings.insert(
        keys::SIZE.to_string(),
        toml::Value::Array(vec![
            toml::Value::Integer(i64::from(drawn.0)),
            toml::Value::Integer(i64::from(drawn.1)),
        ]),
    );
    toml::to_string(&settings).ok()
}

/// An RGBA image as PNG bytes.
#[cfg(test)]
pub(crate) fn png(image: image::RgbaImage) -> Vec<u8> {
    encode(&DynamicImage::ImageRgba8(image), ImageFormat::Png).unwrap()
}

#[cfg(test)]
mod tests {
    use super::{font_pages, png, record_drawn, ship};
    use image::{Rgba, RgbaImage};

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).unwrap()
    }

    fn size_of(bytes: &[u8]) -> (u32, u32) {
        balaur::pixels::size(bytes, &toml::Table::new()).unwrap()
    }

    #[test]
    fn an_image_past_the_cap_ships_smaller_and_remembers_its_size() {
        let source = png(RgbaImage::from_pixel(400, 100, Rgba([9, 9, 9, 255])));
        let shipped = ship(&source, &table(""), 200, false).unwrap().unwrap();
        assert_eq!(size_of(&shipped.bytes), (200, 50));
        assert_eq!(shipped.drawn, Some((400, 100)));
    }

    #[test]
    fn an_image_inside_the_cap_is_kept() {
        let source = png(RgbaImage::new(64, 64));
        assert!(ship(&source, &table(""), 200, false).unwrap().is_none());
        assert!(ship(&source, &table(""), 0, false).unwrap().is_none());
    }

    /// Each of these counts its pixels, so a smaller copy would break it.
    #[test]
    fn pixel_art_a_font_page_and_a_kept_file_are_never_capped() {
        let source = png(RgbaImage::new(400, 400));
        let nearest = table("filter = \"nearest\"");
        assert!(ship(&source, &nearest, 100, false).unwrap().is_none());
        assert!(ship(&source, &table(""), 100, true).unwrap().is_none());
        let kept = table("recode = \"original\"");
        assert!(ship(&source, &kept, 100, false).unwrap().is_none());
    }

    #[test]
    fn a_recorded_size_is_not_overwritten() {
        assert!(record_drawn(Some("size = [8, 4]"), (4, 2)).is_none());
        let written = record_drawn(Some("filter = \"linear\""), (8, 4)).unwrap();
        assert!(written.contains("size = [8, 4]") && written.contains("filter"));
    }

    #[test]
    fn a_font_page_is_found_beside_its_descriptor() {
        let fnt = b"info size=8\ncommon lineHeight=8\npage id=0 file=\"pixel_0.png\"\nchar id=65 x=0 y=0 width=4 height=4 xadvance=4\n";
        let pages = font_pages([("fonts/pixel.fnt", fnt.as_slice())].into_iter());
        assert!(pages.contains("fonts/pixel_0.png"), "{pages:?}");
    }

    #[test]
    fn an_svg_ships_as_a_raster_at_its_scale_and_under_the_cap() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="#00ff00"/></svg>"##;
        let shipped = ship(svg, &table("scale = 2"), 0, false).unwrap().unwrap();
        assert_eq!(size_of(&shipped.bytes), (200, 100));
        assert_eq!(shipped.drawn, None);
        let capped = ship(svg, &table("scale = 2"), 50, false).unwrap().unwrap();
        assert_eq!(size_of(&capped.bytes), (50, 25));
        assert_eq!(capped.drawn, Some((200, 100)));
    }
}
