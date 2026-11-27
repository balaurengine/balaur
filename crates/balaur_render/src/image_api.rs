//! `image.*`: a picture's bytes decoded, turned upright, fitted inside a
//! size and written again as PNG, JPEG or WebP. What a photo the player
//! picked goes through before it is uploaded as an avatar.

use anyhow::{Result, anyhow};
use balaur_asset::pixels::MAX_SIDE;
use balaur_core::Engine;
use balaur_core::handler::{into_bytes, opt, side_pixels};
use balaur_script::{Module, Value};

/// The script module.
pub(crate) const IMAGE: &str = "image";

const ENCODE: &str = "encode";
const ENCODE_ASYNC: &str = "encode_async";
const FORMAT: &str = "format";
const MAX_SIDE_PIXELS: &str = "max_side_pixels";
const QUALITY: &str = "quality";

/// The formats `encode` writes, as the `FORMAT_*` constants hold them.
const PNG: &str = "png";
const JPEG: &str = "jpeg";
const WEBP: &str = "webp";
const FORMATS: [(&str, Format); 3] = [
    (PNG, Format::Png),
    (JPEG, Format::Jpeg),
    (WEBP, Format::Webp),
];

/// What a call left out: a JPEG at nine tenths, as a browser's canvas writes one.
const DEFAULT_QUALITY: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    Png,
    Jpeg,
    Webp,
}

/// One `encode`'s options, read before any work starts.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Asked {
    format: Format,
    max_side: Option<u32>,
    /// JPEG's quality, 1 to 100.
    quality: u8,
}

fn asked(opts: Option<&Value>) -> Result<Asked> {
    let format = match opt(opts, FORMAT) {
        None | Some(Value::Nil) => Format::Jpeg,
        Some(Value::Str(word)) => FORMATS
            .iter()
            .find(|(name, _)| name == word)
            .map(|(_, format)| *format)
            .ok_or_else(|| anyhow!("`format` is an image::FORMAT_* constant, got {word:?}"))?,
        Some(other) => {
            return Err(anyhow!(
                "`format` is an image::FORMAT_* constant, got {other:?}"
            ));
        }
    };
    let max_side = side_pixels(opts, MAX_SIDE_PIXELS, MAX_SIDE)?;
    let quality = match opt(opts, QUALITY) {
        None | Some(Value::Nil) => DEFAULT_QUALITY,
        Some(Value::Num(quality)) => *quality,
        #[allow(clippy::cast_precision_loss, reason = "0 or 1")]
        Some(Value::Int(quality)) => *quality as f64,
        Some(other) => return Err(anyhow!("`quality` is 0 to 1, got {other:?}")),
    };
    if !(0.0..=1.0).contains(&quality) {
        return Err(anyhow!("`quality` is 0 to 1, got {quality}"));
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "0 to 1 scaled to 1 to 100"
    )]
    let quality = (quality * 100.0).round().clamp(1.0, 100.0) as u8;
    Ok(Asked {
        format,
        max_side,
        quality,
    })
}

/// `bytes` decoded, turned as its EXIF orientation says, fitted inside
/// `max_side` on its longest side and written in `format`.
fn encoded(bytes: &[u8], asked: Asked) -> image::ImageResult<Vec<u8>> {
    let picture = balaur_asset::pixels::upright(bytes, asked.max_side)?;
    let mut out = Vec::new();
    match asked.format {
        Format::Png => picture.write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))?,
        // JPEG has no alpha: a transparent pixel is written as its colour.
        Format::Jpeg => image::DynamicImage::ImageRgb8(picture.into_rgb8()).write_with_encoder(
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, asked.quality),
        )?,
        Format::Webp => image::DynamicImage::ImageRgba8(picture.into_rgba8())
            .write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut out))?,
    }
    Ok(out)
}

/// What a script receives: the bytes, or nil for bytes that are no picture.
fn answer(bytes: &[u8], asked: Asked) -> Value {
    encoded(bytes, asked).map_or(Value::Nil, Value::Bytes)
}

pub(crate) fn install(m: &mut Module<Engine>) {
    m.module_doc(
        "A picture's bytes, PNG, JPEG, WebP, GIF or BMP, decoded, turned upright as its EXIF orientation says, fitted inside `max_side_pixels` and written again: what a photo from `device::pick_photo` goes through before an upload. `encode_async` does the work off the frame.",
    );
    for (name, word) in [
        ("FORMAT_PNG", PNG),
        ("FORMAT_JPEG", JPEG),
        ("FORMAT_WEBP", WEBP),
    ] {
        m.constant(name, Value::Str(word.into()));
    }
    m.records(&[(
        "EncodeOptions",
        "{ format: string?, max_side_pixels: int?, quality: float? }",
        "What `encode` writes: a `FORMAT_*` constant (JPEG unless named), the longest side in pixels the picture is shrunk to fit with its proportions kept (never grown), and JPEG's `quality`, 0 to 1 (0.9 unless named). WebP is written lossless.",
    )]);
    m.describe(&[
        (
            ENCODE,
            &[],
            "(bytes: [int], opts: EncodeOptions?) -> [int]?",
            "The picture `bytes` hold, written as `opts` asks, or nil where they are no picture this reads. A photo takes tens of milliseconds: call `encode_async` from a frame that has to stay smooth.",
        ),
        (
            ENCODE_ASYNC,
            &[],
            "(bytes: [int], opts: EncodeOptions?) -> int",
            "`encode` on a worker: the id `task::wait` answers the bytes on, or nil, once it is done.",
        ),
    ]);
    m.function(
        ENCODE,
        |_: &Engine, (bytes, opts): (Value, Option<Value>)| {
            let asked = asked(opts.as_ref())?;
            Ok(answer(&into_bytes(bytes)?, asked))
        },
    );
    m.function(
        ENCODE_ASYNC,
        |eng: &Engine, (bytes, opts): (Value, Option<Value>)| {
            let asked = asked(opts.as_ref())?;
            let bytes = into_bytes(bytes)?;
            Ok(balaur_core::task::off_frame(eng, move || {
                answer(&bytes, asked)
            }))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([200, 40, 40, 255]),
        ))
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
        .unwrap();
        out
    }

    fn size(bytes: &[u8]) -> (u32, u32, image::ImageFormat) {
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .unwrap();
        let format = reader.format().unwrap();
        let decoded = reader.decode().unwrap();
        (decoded.width(), decoded.height(), format)
    }

    fn options(pairs: &[(&str, Value)]) -> Value {
        Value::Map(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn a_picture_is_fitted_inside_its_longest_side_with_its_proportions_kept() {
        let asked = asked(Some(&options(&[
            (MAX_SIDE_PIXELS, Value::Int(100)),
            (FORMAT, Value::text(JPEG)),
        ])))
        .unwrap();
        let (width, height, format) = size(&encoded(&picture(400, 200), asked).unwrap());
        assert_eq!((width, height, format), (100, 50, image::ImageFormat::Jpeg));
    }

    #[test]
    fn a_picture_smaller_than_the_side_keeps_its_size_in_each_format() {
        for (word, format) in [
            (PNG, image::ImageFormat::Png),
            (WEBP, image::ImageFormat::WebP),
            (JPEG, image::ImageFormat::Jpeg),
        ] {
            let asked = asked(Some(&options(&[
                (MAX_SIDE_PIXELS, Value::Int(512)),
                (FORMAT, Value::text(word)),
            ])))
            .unwrap();
            assert_eq!(
                size(&encoded(&picture(64, 32), asked).unwrap()),
                (64, 32, format)
            );
        }
    }

    #[test]
    fn bytes_that_are_no_picture_answer_nil() {
        assert_eq!(answer(&[1, 2, 3], asked(None).unwrap()), Value::Nil);
    }

    #[test]
    fn options_out_of_range_are_refused() {
        for bad in [
            options(&[(QUALITY, Value::Num(1.5))]),
            options(&[(MAX_SIDE_PIXELS, Value::Int(0))]),
            options(&[(FORMAT, Value::text("heic"))]),
        ] {
            assert!(asked(Some(&bad)).is_err(), "{bad:?}");
        }
    }
}
