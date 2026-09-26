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

/// imagequant's quality target, on its own 0-100 scale, for `Quantized`.
pub const DEFAULT_IMAGES_QUALITY: u8 = 80;

/// libvorbis's quality for `Vorbis`, where 0.5 is about 80 kbit/s in stereo.
pub const DEFAULT_AUDIO_QUALITY: f32 = 0.5;

/// How an image entry is re-encoded. Every mode keeps the image's dimensions;
/// `Quantized` is the only one that changes what a pixel says.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageMode {
    /// Ship the author's bytes.
    #[default]
    Keep,
    /// Write the pixels as lossless WebP.
    Webp,
    /// Cut the image to a 256-colour palette with alpha, then write it as a
    /// PNG. Lossy.
    Quantized,
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
/// `quality` is imagequant's 0-100 target, which only `Quantized` reads. A
/// JPEG is always kept: re-encoding one loses a second time.
pub fn image_at(bytes: &[u8], mode: ImageMode, quality: u8) -> Result<Option<Vec<u8>>> {
    let format = image::guess_format(bytes).map_err(|why| anyhow!("reading the image: {why}"))?;
    if mode == ImageMode::Keep || format == ImageFormat::Jpeg {
        return Ok(None);
    }
    let candidate = match mode {
        ImageMode::Keep => None,
        ImageMode::Webp => to_webp(bytes, format)?,
        ImageMode::Quantized => quantize(bytes, format, quality)?,
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
/// The palette is written back out as RGBA rather than indexed: the re-encode
/// still carries far fewer distinct colours, which is where the saving is.
fn quantize(bytes: &[u8], format: ImageFormat, quality: u8) -> Result<Option<Vec<u8>>> {
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
        .map_err(|why| anyhow!("reading the image to quantize: {why}"))?;
    let mut palette = attributes
        .quantize(&mut handle)
        .map_err(|why| anyhow!("quantizing the image: {why}"))?;
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
        .ok_or_else(|| anyhow!("the quantized image lost its dimensions"))?;
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(mapped)
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|why| anyhow!("encoding the quantized PNG: {why}"))?;
    Ok(Some(out))
}

/// What a sound's import settings say about its channels and its rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioShape {
    /// Mix every channel into one.
    pub mono: bool,
    /// The highest sample rate it ships at, in Hz; 0 keeps its own.
    pub max_rate: u32,
}

/// The WAV mixed down to one channel and resampled to at most `max_rate`,
/// written back as a WAV in its own sample format, or `None` when it already
/// fits. Only uncompressed PCM is read: a compressed stream is left alone.
pub fn shape_audio(bytes: &[u8], shape: AudioShape) -> Result<Option<Vec<u8>>> {
    if !is_wav(bytes) || (!shape.mono && shape.max_rate == 0) {
        return Ok(None);
    }
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|why| anyhow!("reading the WAV: {why}"))?;
    let spec = reader.spec();
    let lanes = usize::from(spec.channels.max(1));
    let to_mono = shape.mono && lanes > 1;
    let rate = if shape.max_rate > 0 {
        spec.sample_rate.min(shape.max_rate)
    } else {
        spec.sample_rate
    };
    if !to_mono && rate == spec.sample_rate {
        return Ok(None);
    }
    let mut samples = wav_floats(&mut reader, spec)?;
    samples.truncate(samples.len() / lanes * lanes);
    let (samples, lanes) = if to_mono {
        let mixed = samples
            .chunks_exact(lanes)
            .map(|frame| frame.iter().sum::<f32>() / lanes as f32)
            .collect();
        (mixed, 1)
    } else {
        (samples, lanes)
    };
    let samples = if rate == spec.sample_rate {
        samples
    } else {
        resample(&samples, lanes, spec.sample_rate, rate)?
    };
    let out = hound::WavSpec {
        channels: lanes as u16,
        sample_rate: rate,
        ..spec
    };
    write_wav(&samples, out).map(Some)
}

/// Interleaved floats from one rate to another, through an FFT resampler
/// with an anti-aliasing filter.
fn resample(samples: &[f32], lanes: usize, from: u32, to: u32) -> Result<Vec<f32>> {
    use rubato::audioadapter_buffers::direct::InterleavedSlice;
    use rubato::{Fft, FixedSync, Resampler};
    let frames = samples.len() / lanes;
    if frames == 0 {
        return Ok(Vec::new());
    }
    let mut resampler = Fft::<f32>::new(from as usize, to as usize, 1024, lanes, FixedSync::Both)
        .map_err(|why| anyhow!("building the resampler: {why}"))?;
    let input = InterleavedSlice::new(samples, lanes, frames)
        .map_err(|why| anyhow!("reading the samples to resample: {why}"))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|why| anyhow!("resampling: {why}"))?;
    Ok(output.take_data())
}

/// Interleaved floats as a WAV in `spec`'s own sample format.
fn write_wav(samples: &[f32], spec: hound::WavSpec) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut writer = hound::WavWriter::new(std::io::Cursor::new(&mut out), spec)
        .map_err(|why| anyhow!("starting the WAV: {why}"))?;
    let write = |why| anyhow!("writing the WAV: {why}");
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for &sample in samples {
                writer.write_sample(sample).map_err(write)?;
            }
        }
        hound::SampleFormat::Int => {
            let full = 2f32.powi(i32::from(spec.bits_per_sample) - 1);
            for &sample in samples {
                let level = (sample * full).round().clamp(-full, full - 1.0) as i32;
                writer.write_sample(level).map_err(write)?;
            }
        }
    }
    writer.finalize().map_err(write)?;
    Ok(out)
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
/// sample format was, because libvorbis and the resampler work in floats.
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
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of the encoder this stands in for"
)]
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
#[allow(
    clippy::unnecessary_wraps,
    reason = "the signature of the encoder this stands in for"
)]
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
        let out = image(&source, ImageMode::Webp)
            .unwrap()
            .expect("something smaller");
        assert!(out.len() < source.len(), "WebP grew the image");
        assert_eq!(read_rgba(&out), (width, height, pixels));
    }

    #[test]
    fn keeping_an_image_returns_the_authors_bytes() {
        let source = sample_png(16, 16);
        assert_eq!(image(&source, ImageMode::Keep).unwrap(), None);
    }

    #[test]
    fn a_jpeg_is_never_re_encoded() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpeg.extend_from_slice(&[0u8; 64]);
        assert_eq!(image(&jpeg, ImageMode::Webp).unwrap(), None);
    }

    #[test]
    fn bytes_that_are_not_an_image_are_an_error() {
        assert!(image(b"not an image at all", ImageMode::Webp).is_err());
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

    /// Stereo at 44.1 kHz, one channel a sine and the other silent.
    fn stereo_wav() -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut out = Vec::new();
        let mut writer = hound::WavWriter::new(std::io::Cursor::new(&mut out), spec).unwrap();
        for n in 0..44_100u32 {
            let phase = f64::from(n) * 440.0 * std::f64::consts::TAU / 44_100.0;
            writer
                .write_sample((libm::sin(phase) * 8000.0) as i16)
                .unwrap();
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();
        out
    }

    /// Mixed to one channel at half the rate: a quarter of the samples, the
    /// same second of sound, and the sine at half its level.
    #[test]
    fn a_sound_is_mixed_down_and_resampled_at_export() {
        let shape = AudioShape {
            mono: true,
            max_rate: 22_050,
        };
        let out = shape_audio(&stereo_wav(), shape)
            .unwrap()
            .expect("a shaped WAV");
        let reader = hound::WavReader::new(std::io::Cursor::new(&out)).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (1, 22_050, 16)
        );
        let samples: Vec<i16> = reader.into_samples::<i16>().map(Result::unwrap).collect();
        assert!(
            samples.len().abs_diff(22_050) < 64,
            "{} samples",
            samples.len()
        );
        let loudest = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!((3_500..4_500).contains(&loudest), "{loudest}");
    }

    /// A sound already inside the shape asked for is the author's bytes.
    #[test]
    fn a_sound_that_already_fits_is_kept() {
        let mono_low = AudioShape {
            mono: true,
            max_rate: 48_000,
        };
        assert_eq!(shape_audio(&sample_wav(), mono_low).unwrap(), None);
        assert_eq!(shape_audio(b"OggS not a wav", mono_low).unwrap(), None);
        assert_eq!(
            shape_audio(&stereo_wav(), AudioShape::default()).unwrap(),
            None
        );
    }

    /// The samples symphonia reads back, decoding what a player decodes.
    fn decode_flac(bytes: &[u8]) -> Vec<i32> {
        use symphonia::core::audio::{Audio, GenericAudioBufferRef};
        use symphonia::core::codecs::audio::AudioDecoderOptions;

        let (mut format, params) = probe_audio(bytes, "flac");
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .unwrap();

        let mut samples = Vec::new();
        while let Ok(Some(packet)) = format.next_packet() {
            match decoder.decode(&packet).unwrap() {
                GenericAudioBufferRef::S32(buf) => {
                    samples.extend_from_slice(buf.plane(0).unwrap());
                }
                other => panic!("unexpected sample format {:?}", other.spec()),
            }
        }
        samples
    }

    /// The container's reader and its audio codec parameters. The parameters
    /// are cloned so the reader is free to be borrowed for packets after.
    fn probe_audio(
        bytes: &[u8],
        extension: &str,
    ) -> (
        Box<dyn symphonia::core::formats::FormatReader>,
        symphonia::core::codecs::audio::AudioCodecParameters,
    ) {
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::{FormatOptions, TrackType};
        use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
        use symphonia::core::meta::MetadataOptions;

        let source = std::io::Cursor::new(bytes.to_vec());
        let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        hint.with_extension(extension);
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .unwrap();
        let params = format
            .default_track(TrackType::Audio)
            .and_then(|t| t.codec_params.as_ref())
            .and_then(|p| p.audio())
            .expect("an audio track")
            .clone();
        (format, params)
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

    /// How many frames symphonia reads back out of an Ogg Vorbis stream.
    fn decode_ogg_frames(bytes: &[u8]) -> u64 {
        use symphonia::core::codecs::audio::AudioDecoderOptions;

        let (mut format, params) = probe_audio(bytes, "ogg");
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .unwrap();

        let mut frames = 0;
        while let Ok(Some(packet)) = format.next_packet() {
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
