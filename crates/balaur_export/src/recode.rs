//! Re-encoding one exported entry smaller without changing what it says.
//!
//! Every function here is pure: bytes in, bytes out. Nothing reads the
//! filesystem, nothing knows about a pack, and nothing decides policy — the
//! caller holds the `[export]` keys and the paths.
//!
//! `Ok(None)` is the answer whenever nothing smaller was found, so a caller
//! that keeps the original on `None` can never ship a bigger file than the
//! author wrote. `Err` means the input did not decode.
//!
//! A re-encode never changes an image's pixel dimensions: a sprite reads its
//! extent from the image header, so a resize would move the simulation.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use image::{DynamicImage, ImageFormat};
use image_webp::{ColorType, WebPEncoder};
use serde::Deserialize;

/// How an image entry is re-encoded. Every mode is lossless and keeps the
/// image's dimensions; a lossy palette or a downscale is another key's.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageMode {
    /// Ship the author's bytes.
    #[default]
    Keep,
    /// Run `oxipng` over a PNG.
    Png,
    /// Write the pixels as lossless WebP.
    Webp,
    /// Try both and keep whichever won.
    Smallest,
}

/// Whether a face ships whole or cut down to the code points a game shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontMode {
    /// Ship every glyph the author's face carries.
    #[default]
    Keep,
    /// Keep only the code points asked for, with the layout tables.
    Subset,
}

/// How a sound entry is re-encoded. FLAC is lossless; a lossy stream is
/// another key's.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioMode {
    /// Ship the author's bytes.
    #[default]
    Keep,
    /// Re-encode uncompressed PCM as FLAC.
    Flac,
}

/// What one entry weighed before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saving {
    /// The pack key, which a re-encode never changes.
    pub path: String,
    /// The source's bytes.
    pub before: usize,
    /// The re-encoded bytes, or `before` again where nothing was found.
    pub after: usize,
}

impl Saving {
    /// The bytes this entry no longer costs.
    pub fn saved(&self) -> usize {
        self.before.saturating_sub(self.after)
    }
}

/// The image re-encoded under `mode`, or `None` to keep the source bytes.
///
/// A JPEG is always kept: re-encoding one loses a second time.
pub fn image(bytes: &[u8], mode: ImageMode) -> Result<Option<Vec<u8>>> {
    let format = image::guess_format(bytes).map_err(|why| anyhow!("reading the image: {why}"))?;
    if mode == ImageMode::Keep || format == ImageFormat::Jpeg {
        return Ok(None);
    }
    let candidate = match mode {
        ImageMode::Keep => None,
        ImageMode::Png => shrink_png(bytes, format)?,
        ImageMode::Webp => to_webp(bytes, format)?,
        ImageMode::Smallest => smaller_of(shrink_png(bytes, format)?, to_webp(bytes, format)?),
    };
    Ok(candidate.filter(|out| out.len() < bytes.len()))
}

/// The face carrying only `keep`'s code points, or `None` to ship it whole.
///
/// Without the `recode-fonts` feature there is no subsetter, so every face is
/// kept whole and a target with no C++ toolchain still exports.
pub fn font(bytes: &[u8], keep: &BTreeSet<char>) -> Result<Option<Vec<u8>>> {
    subset_face(bytes, keep)
}

/// The sound re-encoded under `mode`, or `None` to keep the source bytes.
///
/// Only uncompressed PCM in a WAV is touched; an already-compressed stream is
/// left alone.
pub fn audio(bytes: &[u8], mode: AudioMode) -> Result<Option<Vec<u8>>> {
    if mode == AudioMode::Keep || !is_wav(bytes) {
        return Ok(None);
    }
    let candidate = to_flac(bytes)?;
    Ok(candidate.filter(|out| out.len() < bytes.len()))
}

/// Whichever of two candidates is smaller.
fn smaller_of(one: Option<Vec<u8>>, other: Option<Vec<u8>>) -> Option<Vec<u8>> {
    match (one, other) {
        (Some(a), Some(b)) => Some(if b.len() < a.len() { b } else { a }),
        (found, None) | (None, found) => found,
    }
}

/// oxipng over a PNG's own bytes, which keeps every pixel and the dimensions.
fn shrink_png(bytes: &[u8], format: ImageFormat) -> Result<Option<Vec<u8>>> {
    if format != ImageFormat::Png {
        return Ok(None);
    }
    let mut options = oxipng::Options::max_compression();
    // Zopfli's deflate is the whole point of the pass; `Safe` drops only
    // chunks that cannot affect what the image looks like.
    options.deflater = oxipng::Deflater::Zopfli(oxipng::ZopfliOptions::default());
    options.strip = oxipng::StripChunks::Safe;
    let out = oxipng::optimize_from_memory(bytes, &options)
        .map_err(|why| anyhow!("optimizing the PNG: {why}"))?;
    Ok(Some(out))
}

/// The image's pixels as lossless WebP, at the source's dimensions.
fn to_webp(bytes: &[u8], format: ImageFormat) -> Result<Option<Vec<u8>>> {
    // A source that is already WebP gains nothing from this encoder, and no
    // other format decodes in a build carrying only `image`'s `png` feature.
    if format != ImageFormat::Png {
        return Ok(None);
    }
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|why| anyhow!("decoding the PNG: {why}"))?;
    let (width, height) = (decoded.width(), decoded.height());
    let (pixels, color) = match decoded {
        DynamicImage::ImageLuma8(img) => (img.into_raw(), ColorType::L8),
        DynamicImage::ImageLumaA8(img) => (img.into_raw(), ColorType::La8),
        DynamicImage::ImageRgb8(img) => (img.into_raw(), ColorType::Rgb8),
        DynamicImage::ImageRgba8(img) => (img.into_raw(), ColorType::Rgba8),
        // WebP carries eight bits a channel; a deeper source would lose samples.
        _ => return Ok(None),
    };
    let mut out = Vec::new();
    WebPEncoder::new(&mut out)
        .encode(&pixels, width, height, color)
        .map_err(|why| anyhow!("encoding the WebP: {why}"))?;
    Ok(Some(out))
}

/// A RIFF/WAVE header, which is the only sound this module re-encodes.
fn is_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE"
}

/// The WAV's samples as FLAC, sample for sample.
fn to_flac(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|why| anyhow!("reading the WAV: {why}"))?;
    let spec = reader.spec();
    // FLAC stores signed integers of at most 24 bits a sample in eight
    // channels; anything wider or floating would have to be rounded.
    if spec.sample_format != hound::SampleFormat::Int
        || !matches!(spec.bits_per_sample, 8 | 12 | 16 | 20 | 24)
        || !(1..=8).contains(&spec.channels)
    {
        return Ok(None);
    }
    let samples = reader
        .samples::<i32>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|why| anyhow!("reading the WAV's samples: {why}"))?;
    if samples.is_empty() {
        return Ok(None);
    }

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, why)| anyhow!("the FLAC encoder's configuration: {why}"))?;
    let source = flacenc::source::MemSource::from_samples(
        &samples,
        usize::from(spec.channels),
        usize::from(spec.bits_per_sample),
        spec.sample_rate as usize,
    );
    let mut stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|why| anyhow!("encoding the FLAC: {why}"))?;
    // flacenc reports the last, short frame as the stream's minimum block
    // size, which makes a fixed-block-size stream look variable; symphonia
    // then refuses to sync to a frame. libFLAC writes min == max here.
    let longest = stream.stream_info().max_block_size();
    stream
        .stream_info_mut()
        .set_block_sizes(longest, longest)
        .map_err(|why| anyhow!("the FLAC stream's block sizes: {why}"))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|why| anyhow!("writing the FLAC: {why}"))?;
    Ok(Some(sink.into_inner()))
}

#[cfg(feature = "recode-fonts")]
fn subset_face(bytes: &[u8], keep: &BTreeSet<char>) -> Result<Option<Vec<u8>>> {
    if keep.is_empty() {
        return Ok(None);
    }
    let blob = hb_subset::Blob::from_bytes(bytes)?;
    let face = hb_subset::FontFace::new(blob)?;
    let mut input = hb_subset::SubsetInput::new()?;
    for &point in keep {
        input.unicode_set().insert(point);
    }
    // cosmic-text shapes through rustybuzz, which reads GSUB and GPOS: `*`
    // keeps every layout feature rather than HarfBuzz's default shortlist, so
    // kerning, ligatures and Arabic joining survive the cut.
    input
        .layout_feature_tag_set()
        .insert(hb_subset::Tag::new(b"*   "));
    let subset = input.subset_font(&face)?;
    let out = subset.underlying_blob().to_vec();
    Ok(Some(out).filter(|out| out.len() < bytes.len()))
}

#[cfg(not(feature = "recode-fonts"))]
fn subset_face(_bytes: &[u8], _keep: &BTreeSet<char>) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gradient with a varying alpha channel: enough structure that an
    /// encoder can win, enough alpha that a lost channel would show.
    fn sample_png(width: u32, height: u32) -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([
                (x * 4 % 256) as u8,
                (y * 4 % 256) as u8,
                ((x + y) % 256) as u8,
                (x % 2 * 255) as u8,
            ])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    /// Any image this module produces, read back as RGBA8 with its size.
    fn read_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        if image::guess_format(bytes).unwrap() == ImageFormat::Png {
            let img = image::load_from_memory(bytes).unwrap().to_rgba8();
            return (img.width(), img.height(), img.into_raw());
        }
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).unwrap();
        let (width, height) = decoder.dimensions();
        let mut buffer = vec![0u8; decoder.output_buffer_size().unwrap()];
        decoder.read_image(&mut buffer).unwrap();
        if decoder.has_alpha() {
            return (width, height, buffer);
        }
        let rgba = buffer
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        (width, height, rgba)
    }

    #[test]
    fn a_recoded_image_keeps_its_pixels_and_its_size() {
        let source = sample_png(64, 48);
        let (width, height, pixels) = read_rgba(&source);
        assert_eq!((width, height), (64, 48));
        for mode in [ImageMode::Png, ImageMode::Webp, ImageMode::Smallest] {
            let out = image(&source, mode).unwrap().expect("something smaller");
            assert!(out.len() < source.len(), "{mode:?} grew the image");
            assert_eq!(read_rgba(&out), (width, height, pixels.clone()), "{mode:?}");
        }
    }

    #[test]
    fn keeping_an_image_returns_the_authors_bytes() {
        let source = sample_png(16, 16);
        assert_eq!(image(&source, ImageMode::Keep).unwrap(), None);
    }

    #[test]
    fn an_image_with_nothing_left_to_save_is_left_alone() {
        let source = sample_png(64, 48);
        let once = image(&source, ImageMode::Png).unwrap().expect("smaller");
        // A second pass finds the same bytes, which are not smaller than the
        // first pass's: no candidate may replace an entry it did not shrink.
        assert_eq!(image(&once, ImageMode::Png).unwrap(), None);
    }

    #[test]
    fn a_jpeg_is_never_re_encoded() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&[0u8; 64]);
        assert_eq!(image(&jpeg, ImageMode::Smallest).unwrap(), None);
    }

    #[test]
    fn bytes_that_are_not_an_image_are_an_error() {
        assert!(image(b"not an image at all", ImageMode::Smallest).is_err());
    }

    /// One second of a quiet sine, the shape a game's sound effect has.
    fn sample_wav() -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut out = Vec::new();
        let mut writer = hound::WavWriter::new(std::io::Cursor::new(&mut out), spec).unwrap();
        for n in 0..22_050u32 {
            let phase = f64::from(n) * 440.0 * std::f64::consts::TAU / 22_050.0;
            writer.write_sample((phase.sin() * 8000.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
        out
    }

    /// The samples symphonia reads back, which is the decoder the runtime uses.
    fn decode_flac(bytes: &[u8]) -> Vec<i32> {
        use symphonia::core::audio::{AudioBufferRef, Signal};
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let source = std::io::Cursor::new(bytes.to_vec());
        let stream = MediaSourceStream::new(Box::new(source), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("flac");
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .unwrap();
        let mut format = probed.format;
        let track = format.default_track().unwrap();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .unwrap();

        let mut samples = Vec::new();
        while let Ok(packet) = format.next_packet() {
            match decoder.decode(&packet).unwrap() {
                AudioBufferRef::S32(buf) => samples.extend_from_slice(buf.chan(0)),
                other => panic!("unexpected sample format {:?}", other.spec()),
            }
        }
        samples
    }

    #[test]
    fn a_wav_becomes_a_smaller_flac_with_the_same_samples() {
        let source = sample_wav();
        let flac = audio(&source, AudioMode::Flac).unwrap().expect("a FLAC");
        assert!(
            flac.len() < source.len(),
            "{} vs {}",
            flac.len(),
            source.len()
        );

        let mut reader = hound::WavReader::new(std::io::Cursor::new(&source)).unwrap();
        let expected: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
        // symphonia hands FLAC back left-aligned in 32 bits; the WAV's are 16.
        let decoded: Vec<i32> = decode_flac(&flac).iter().map(|s| s >> 16).collect();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn keeping_a_sound_returns_the_authors_bytes() {
        assert_eq!(audio(&sample_wav(), AudioMode::Keep).unwrap(), None);
    }

    #[test]
    fn a_sound_that_is_not_a_wav_is_left_alone() {
        assert_eq!(
            audio(b"OggS\0\0\0\0\0\0\0\0", AudioMode::Flac).unwrap(),
            None
        );
    }

    /// The tags in a face's table directory, so a test can say GPOS survived.
    fn table_tags(face: &[u8]) -> BTreeSet<String> {
        let count = u16::from_be_bytes([face[4], face[5]]) as usize;
        (0..count)
            .map(|i| {
                let at = 12 + i * 16;
                String::from_utf8_lossy(&face[at..at + 4]).into_owned()
            })
            .collect()
    }

    fn ui_face() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../editor/fonts/ui-SourceSans3-Regular.ttf");
        std::fs::read(path).unwrap()
    }

    #[test]
    #[cfg(feature = "recode-fonts")]
    fn a_subset_face_keeps_the_glyphs_it_was_given_and_its_layout_tables() {
        let source = ui_face();
        let keep: BTreeSet<char> = "Hello, world! fi".chars().collect();
        let subset = font(&source, &keep).unwrap().expect("a smaller face");
        assert!(subset.len() < source.len() / 2, "{} bytes", subset.len());

        let blob = hb_subset::Blob::from_bytes(&subset).unwrap();
        let face = hb_subset::FontFace::new(blob).unwrap();
        let covered = face.covered_codepoints().unwrap();
        for point in &keep {
            assert!(covered.contains(*point), "{point:?} was dropped");
        }
        let tags = table_tags(&subset);
        assert!(tags.contains("GSUB"), "{tags:?}");
        assert!(tags.contains("GPOS"), "{tags:?}");
    }

    #[test]
    #[cfg(feature = "recode-fonts")]
    fn a_face_asked_to_keep_nothing_is_shipped_whole() {
        assert_eq!(font(&ui_face(), &BTreeSet::new()).unwrap(), None);
    }

    #[test]
    #[cfg(not(feature = "recode-fonts"))]
    fn without_the_subsetter_every_face_is_shipped_whole() {
        let keep: BTreeSet<char> = "abc".chars().collect();
        assert_eq!(font(&ui_face(), &keep).unwrap(), None);
    }
}
