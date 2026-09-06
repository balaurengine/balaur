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

/// imagequant's quality target, on its own 0-100 scale, for `Quantised`.
pub const DEFAULT_IMAGES_QUALITY: u8 = 80;

/// libvorbis's quality for `Vorbis`, where 0.5 is about 80 kbit/s in stereo.
pub const DEFAULT_AUDIO_QUALITY: f32 = 0.5;

/// How an image entry is re-encoded. Every mode keeps the image's dimensions;
/// `Quantised` is the only one that changes what a pixel says.
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
    /// Try both and keep whichever won. Lossless: a lossy mode is asked for
    /// by name, never arrived at by a search for the smaller file.
    Smallest,
    /// Cut the image to a 256-colour palette with alpha, then run the PNG
    /// path over it. Lossy.
    Quantised,
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

/// How a sound entry is re-encoded, losslessly or not.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioMode {
    /// Ship the author's bytes.
    #[default]
    Keep,
    /// Re-encode uncompressed PCM as FLAC, sample for sample.
    Flac,
    /// Re-encode uncompressed PCM as Ogg Vorbis, which the runtime decodes
    /// through rodio's symphonia. Lossy.
    Vorbis,
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

/// The image re-encoded under `mode` at the default quality.
pub fn image(bytes: &[u8], mode: ImageMode) -> Result<Option<Vec<u8>>> {
    image_at(bytes, mode, DEFAULT_IMAGES_QUALITY)
}

/// The image re-encoded under `mode`, or `None` to keep the source bytes.
///
/// `quality` is imagequant's 0-100 target, which only `Quantised` reads. A
/// JPEG is always kept: re-encoding one loses a second time.
pub fn image_at(bytes: &[u8], mode: ImageMode, quality: u8) -> Result<Option<Vec<u8>>> {
    let format = image::guess_format(bytes).map_err(|why| anyhow!("reading the image: {why}"))?;
    if mode == ImageMode::Keep || format == ImageFormat::Jpeg {
        return Ok(None);
    }
    let candidate = match mode {
        ImageMode::Keep => None,
        ImageMode::Png => shrink_png(bytes, format)?,
        ImageMode::Webp => to_webp(bytes, format)?,
        ImageMode::Smallest => smaller_of(shrink_png(bytes, format)?, to_webp(bytes, format)?),
        ImageMode::Quantised => quantise(bytes, format, quality)?,
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

/// The sound re-encoded under `mode` at the default quality.
pub fn audio(bytes: &[u8], mode: AudioMode) -> Result<Option<Vec<u8>>> {
    audio_at(bytes, mode, DEFAULT_AUDIO_QUALITY)
}

/// The sound re-encoded under `mode`, or `None` to keep the source bytes.
///
/// `quality` is libvorbis's -0.1 to 1.0 scale, which only `Vorbis` reads.
/// Only uncompressed PCM in a WAV is touched; an already-compressed stream is
/// left alone.
pub fn audio_at(bytes: &[u8], mode: AudioMode, quality: f32) -> Result<Option<Vec<u8>>> {
    if mode == AudioMode::Keep || !is_wav(bytes) {
        return Ok(None);
    }
    let candidate = match mode {
        AudioMode::Keep => None,
        AudioMode::Flac => to_flac(bytes)?,
        AudioMode::Vorbis => to_vorbis(bytes, quality)?,
    };
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
///
/// Absent on wasm, where oxipng's deflate does not build: the WebP path still
/// runs, so `Smallest` in a browser tab means WebP or nothing.
#[cfg(not(target_family = "wasm"))]
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

/// The image cut to a 256-colour palette with alpha, as a PNG.
///
/// The palette is written back out as RGBA and left to the PNG path to index:
/// oxipng reduces a 256-colour image to a palette itself, and where that pass
/// is absent the plain re-encode still carries far fewer distinct colours.
fn quantise(bytes: &[u8], format: ImageFormat, quality: u8) -> Result<Option<Vec<u8>>> {
    if format != ImageFormat::Png {
        return Ok(None);
    }
    let source = image::load_from_memory_with_format(bytes, format)
        .map_err(|why| anyhow!("decoding the PNG: {why}"))?
        .to_rgba8();
    let (width, height) = (source.width(), source.height());
    let pixels: Vec<imagequant::RGBA> = source
        .pixels()
        .map(|p| imagequant::RGBA::new(p[0], p[1], p[2], p[3]))
        .collect();

    let mut attributes = imagequant::new();
    // A minimum of zero never aborts: whether the palette was worth it is
    // decided by the size filter, not by refusing to encode.
    attributes
        .set_quality(0, quality.min(100))
        .map_err(|why| anyhow!("the palette's quality target: {why}"))?;
    let mut handle = attributes
        .new_image(pixels, width as usize, height as usize, 0.0)
        .map_err(|why| anyhow!("reading the image to quantise: {why}"))?;
    let mut palette = attributes
        .quantize(&mut handle)
        .map_err(|why| anyhow!("quantising the image: {why}"))?;
    let (colours, indices) = palette
        .remapped(&mut handle)
        .map_err(|why| anyhow!("remapping the image onto its palette: {why}"))?;

    let flat: Vec<u8> = indices
        .iter()
        .flat_map(|&at| {
            let colour = colours[usize::from(at)];
            [colour.r, colour.g, colour.b, colour.a]
        })
        .collect();
    let mapped = image::RgbaImage::from_raw(width, height, flat)
        .ok_or_else(|| anyhow!("the quantised image lost its dimensions"))?;
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(mapped)
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|why| anyhow!("encoding the quantised PNG: {why}"))?;
    let shrunk = shrink_png(&out, ImageFormat::Png)?;
    Ok(smaller_of(shrunk, Some(out)))
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

/// The WAV's samples as Ogg Vorbis at libvorbis's quality `quality`.
///
/// Absent on wasm, where libvorbis and libogg do not build: a browser tab
/// ships the author's WAV, as it ships an unshrunk PNG.
#[cfg(not(target_family = "wasm"))]
fn to_vorbis(bytes: &[u8], quality: f32) -> Result<Option<Vec<u8>>> {
    use std::num::{NonZeroU8, NonZeroU32};

    use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

    // libvorbis takes one plane of floats a channel; a block near its own
    // 8192-sample window keeps the encoder's memory and time flat.
    const BLOCK: usize = 4096;

    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|why| anyhow!("reading the WAV: {why}"))?;
    let spec = reader.spec();
    let channel_count = u8::try_from(spec.channels).unwrap_or(0);
    let (Some(rate), Some(channels)) = (
        NonZeroU32::new(spec.sample_rate),
        NonZeroU8::new(channel_count),
    ) else {
        return Ok(None);
    };
    if !(1..=32).contains(&spec.bits_per_sample) {
        return Ok(None);
    }
    let lanes = usize::from(channels.get());
    let mut interleaved = wav_floats(&mut reader, spec)?;
    // libvorbis reads whole frames, and a block of none of them ends the
    // stream early; a WAV whose samples do not divide by its channels has one.
    let frames = interleaved.len() / lanes;
    if frames == 0 {
        return Ok(None);
    }
    interleaved.truncate(frames * lanes);

    let mut out = Vec::new();
    let mut builder = VorbisEncoderBuilder::new(rate, channels, &mut out)
        .map_err(|why| anyhow!("starting the Vorbis encoder: {why}"))?;
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: quality.clamp(-0.1, 1.0),
    });
    let mut encoder = builder
        .build()
        .map_err(|why| anyhow!("building the Vorbis encoder: {why}"))?;
    for block in interleaved.chunks(BLOCK * lanes) {
        let held = block.len() / lanes;
        let planes: Vec<Vec<f32>> = (0..lanes)
            .map(|lane| (0..held).map(|at| block[at * lanes + lane]).collect())
            .collect();
        encoder
            .encode_audio_block(&planes)
            .map_err(|why| anyhow!("encoding the Vorbis stream: {why}"))?;
    }
    encoder
        .finish()
        .map_err(|why| anyhow!("finishing the Vorbis stream: {why}"))?;
    Ok(Some(out))
}

/// A WAV's samples as interleaved floats in -1.0 to 1.0, whatever its own
/// sample format was, because libvorbis analyses floats.
#[cfg(not(target_family = "wasm"))]
fn wav_floats(
    reader: &mut hound::WavReader<std::io::Cursor<&[u8]>>,
    spec: hound::WavSpec,
) -> Result<Vec<f32>> {
    let read = |why| anyhow!("reading the WAV's samples: {why}");
    match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(read),
        hound::SampleFormat::Int => {
            let full = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|sample| sample as f32 / full))
                .collect::<Result<_, _>>()
                .map_err(read)
        }
    }
}

#[cfg(target_family = "wasm")]
fn shrink_png(_bytes: &[u8], _format: ImageFormat) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

#[cfg(target_family = "wasm")]
fn to_vorbis(_bytes: &[u8], _quality: f32) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

#[cfg(all(feature = "recode-fonts", not(target_family = "wasm")))]
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

#[cfg(not(all(feature = "recode-fonts", not(target_family = "wasm"))))]
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
            .as_chunks::<3>()
            .0
            .iter()
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

    /// Far more colours than a palette holds and no structure a lossless
    /// encoder can find, which is what a photograph is to a quantiser.
    fn photo_png(width: u32, height: u32) -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(width, height, |x, y| {
            // An avalanche hash, not a gradient: PNG's own filters subtract a
            // linear one away and leave a palette nothing to beat.
            let mut mixed = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(2_246_822_519);
            mixed ^= mixed >> 15;
            mixed = mixed.wrapping_mul(2_246_822_519);
            mixed ^= mixed >> 13;
            image::Rgba([mixed as u8, (mixed >> 8) as u8, (mixed >> 16) as u8, 255])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_quantised_image_keeps_its_dimensions_and_its_alpha() {
        let source = sample_png(64, 48);
        let out = image(&source, ImageMode::Quantised)
            .unwrap()
            .expect("something smaller");
        let (width, height, pixels) = read_rgba(&out);
        assert_eq!((width, height), (64, 48));
        let alphas: BTreeSet<u8> = pixels.iter().skip(3).step_by(4).copied().collect();
        assert!(
            alphas.contains(&0),
            "transparency was flattened: {alphas:?}"
        );
        assert!(alphas.contains(&255), "opacity was flattened: {alphas:?}");
    }

    #[test]
    fn a_quantised_image_is_smaller_than_the_lossless_one() {
        let source = photo_png(96, 96);
        let lossless = image(&source, ImageMode::Smallest)
            .unwrap()
            .map_or(source.len(), |out| out.len());
        let quantised = image(&source, ImageMode::Quantised)
            .unwrap()
            .expect("something smaller");
        assert!(
            quantised.len() < lossless,
            "{} vs {lossless}",
            quantised.len()
        );
    }

    #[test]
    fn smallest_never_quantises() {
        let source = photo_png(96, 96);
        let wanted = read_rgba(&source);
        let out = image(&source, ImageMode::Smallest)
            .unwrap()
            .unwrap_or_else(|| source.clone());
        assert_eq!(read_rgba(&out), wanted, "Smallest lost a colour");
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
            let sample = (libm::sin(phase) * 8000.0) as i16;
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        out
    }

    /// The samples symphonia reads back, which is the decoder the runtime uses.
    fn decode_flac(bytes: &[u8]) -> Vec<i32> {
        use symphonia::core::audio::{AudioBufferRef, Signal};
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let source = std::io::Cursor::new(bytes.to_vec());
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
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

    /// How many frames symphonia reads back out of an Ogg Vorbis stream,
    /// which is the decoder the runtime uses.
    fn decode_ogg_frames(bytes: &[u8]) -> u64 {
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let source = std::io::Cursor::new(bytes.to_vec());
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        hint.with_extension("ogg");
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

        let mut frames = 0;
        while let Ok(packet) = format.next_packet() {
            frames += decoder.decode(&packet).unwrap().frames() as u64;
        }
        frames
    }

    #[test]
    fn a_wav_becomes_a_smaller_ogg_of_the_same_duration() {
        let source = sample_wav();
        let ogg = audio(&source, AudioMode::Vorbis)
            .unwrap()
            .expect("an Ogg Vorbis stream");
        assert!(
            ogg.len() < source.len(),
            "{} vs {}",
            ogg.len(),
            source.len()
        );

        // Vorbis codes overlapping windows, so a stream carries a lead-in and
        // a tail beyond the samples; a long window is 2048 of them.
        let frames = decode_ogg_frames(&ogg);
        assert!(
            frames.abs_diff(22_050) <= 2048,
            "{frames} frames, not 22050"
        );
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
