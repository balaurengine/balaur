//! Pictures a script makes from bytes it holds (`ui::picture`): a photo the
//! player picked, shown before it is uploaded. Each is named `picture:<n>`,
//! which an `image`'s `image` and a button's `image` draw as they draw a
//! file, until `ui::free_picture` lets it go.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use balaur_asset::pixels::MAX_SIDE;
use balaur_core::Engine;
use balaur_core::handler::{into_bytes, side_pixels};
use balaur_script::{Module, Value};

/// What every picture's name starts with: no file a project ships is named
/// so, since Windows takes no colon in a file's name.
const PREFIX: &str = "picture:";

const PICTURE: &str = "picture";
const PICTURE_ASYNC: &str = "picture_async";
const FREE_PICTURE: &str = "free_picture";
const MAX_SIDE_PIXELS: &str = "max_side_pixels";

/// The pictures made, by name: decoded pixels until a pass uploads them,
/// then the texture.
#[derive(Default)]
pub(crate) struct Pictures {
    made: u64,
    held: HashMap<String, Held>,
    /// What a worker decoded, handed over before the name it answers is heard.
    arriving: Arc<Mutex<Vec<(String, egui::ColorImage)>>>,
}

enum Held {
    Pixels(egui::ColorImage),
    Up(egui::TextureHandle),
}

impl Pictures {
    fn next_name(&mut self) -> String {
        self.made += 1;
        format!("{PREFIX}{}", self.made)
    }

    /// Take in what the workers decoded.
    fn settle(&mut self) {
        let arrived = std::mem::take(
            &mut *self
                .arriving
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for (name, pixels) in arrived {
            self.held.insert(name, Held::Pixels(pixels));
        }
    }
}

/// Whether `source` names a picture made from bytes rather than a file.
pub(crate) fn is_picture(source: &str) -> bool {
    source.starts_with(PREFIX)
}

/// The texture of the picture `name`, uploaded the first time it is drawn.
///
/// # Errors
/// For a name no picture has: never made, or freed.
pub(crate) fn texture(
    eng: &Engine,
    ctx: &egui::Context,
    name: &str,
) -> Result<egui::TextureHandle> {
    let pictures = eng.resource::<Pictures>();
    let mut pictures = pictures.borrow_mut();
    if let Some(Held::Up(texture)) = pictures.held.get(name) {
        return Ok(texture.clone());
    }
    pictures.settle();
    let held = pictures
        .held
        .get_mut(name)
        .ok_or_else(|| anyhow!("no picture is named {name}: it was freed, or never made"))?;
    let texture = match held {
        Held::Up(texture) => texture.clone(),
        Held::Pixels(pixels) => {
            ctx.load_texture(name, std::mem::take(pixels), egui::TextureOptions::LINEAR)
        }
    };
    *held = Held::Up(texture.clone());
    Ok(texture)
}

/// The pixels `bytes` hold, upright and fitted, or `None` for no picture.
fn decoded(bytes: &[u8], max_side: Option<u32>) -> Option<egui::ColorImage> {
    let picture = balaur_asset::pixels::upright(bytes, max_side).ok()?;
    let size = [picture.width() as usize, picture.height() as usize];
    // A photo is most often RGB: read as it is, with no RGBA copy between.
    Some(match picture {
        image::DynamicImage::ImageRgb8(rgb) => egui::ColorImage::from_rgb(size, rgb.as_raw()),
        other => egui::ColorImage::from_rgba_unmultiplied(size, other.into_rgba8().as_raw()),
    })
}

pub(crate) fn install(m: &mut Module<Engine>) {
    m.records(&[(
        "PictureOptions",
        "{ max_side_pixels: int? }",
        "How `picture` reads its bytes: the longest side in pixels the picture is shrunk to fit with its proportions kept (never grown).",
    )]);
    m.describe(&[
        (
            PICTURE,
            &[],
            "(bytes: [int], opts: PictureOptions?) -> string?",
            "A picture from the bytes a script holds, PNG, JPEG, WebP, GIF or BMP, turned upright as its EXIF orientation says: the name an `image`'s `image` or a button's `image` draws it by, `picture:<n>`, held until `free_picture` lets it go. Nil for bytes that are no picture. What shows a photo from `device::pick_photo` before it is uploaded. A photo takes tens of milliseconds: call `picture_async` from a frame that has to stay smooth.",
        ),
        (
            PICTURE_ASYNC,
            &[],
            "(bytes: [int], opts: PictureOptions?) -> int",
            "`picture` decoded on a worker: the id `task::wait` answers the picture's name on, or nil, once it is done. The picture is held from then on, waited on or not.",
        ),
        (
            FREE_PICTURE,
            &[],
            "(name: string) -> bool",
            "Let the picture `picture` named go, its texture with it; a widget still naming it draws its `alt_text`. False for a name no picture has.",
        ),
    ]);
    m.function(
        PICTURE,
        |eng: &Engine, (bytes, opts): (Value, Option<Value>)| {
            let side = side_pixels(opts.as_ref(), MAX_SIDE_PIXELS, MAX_SIDE)?;
            let Some(pixels) = decoded(&into_bytes(bytes)?, side) else {
                return Ok(Value::Nil);
            };
            let pictures = eng.resource::<Pictures>();
            let mut pictures = pictures.borrow_mut();
            let name = pictures.next_name();
            pictures.held.insert(name.clone(), Held::Pixels(pixels));
            Ok(Value::text(name))
        },
    );
    m.function(
        PICTURE_ASYNC,
        |eng: &Engine, (bytes, opts): (Value, Option<Value>)| {
            let side = side_pixels(opts.as_ref(), MAX_SIDE_PIXELS, MAX_SIDE)?;
            let bytes = into_bytes(bytes)?;
            let (name, arriving) = {
                let pictures = eng.resource::<Pictures>();
                let mut pictures = pictures.borrow_mut();
                (pictures.next_name(), pictures.arriving.clone())
            };
            Ok(balaur_core::task::off_frame(eng, move || {
                let Some(pixels) = decoded(&bytes, side) else {
                    return Value::Nil;
                };
                arriving
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((name.clone(), pixels));
                Value::text(name)
            }))
        },
    );
    m.function(FREE_PICTURE, |eng: &Engine, name: String| {
        let pictures = eng.resource::<Pictures>();
        let mut pictures = pictures.borrow_mut();
        pictures.settle();
        Ok(pictures.held.remove(&name).is_some())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            width,
            height,
            image::Rgba([20, 120, 220, 255]),
        ))
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut out))
        .unwrap();
        out
    }

    #[test]
    fn a_picture_is_fitted_inside_its_longest_side() {
        let pixels = decoded(&png(400, 200), Some(100)).unwrap();
        assert_eq!(pixels.size, [100, 50]);
        assert_eq!(decoded(&png(40, 20), Some(100)).unwrap().size, [40, 20]);
    }

    #[test]
    fn bytes_that_are_no_picture_decode_to_none() {
        assert!(decoded(&[1, 2, 3], None).is_none());
    }

    #[test]
    fn a_picture_is_drawn_by_its_name_until_it_is_freed() {
        let eng = Engine::default();
        eng.insert_resource(Pictures::default());
        let ctx = egui::Context::default();
        let name = {
            let pictures = eng.resource::<Pictures>();
            let mut pictures = pictures.borrow_mut();
            let name = pictures.next_name();
            pictures
                .arriving
                .lock()
                .unwrap()
                .push((name.clone(), decoded(&png(8, 4), None).unwrap()));
            name
        };
        assert!(is_picture(&name), "{name}");
        let texture = texture(&eng, &ctx, &name).unwrap();
        assert_eq!(texture.size(), [8, 4]);
        assert_eq!(
            super::texture(&eng, &ctx, &name).unwrap().id(),
            texture.id(),
            "uploaded once"
        );
        assert!(
            eng.resource::<Pictures>()
                .borrow_mut()
                .held
                .remove(&name)
                .is_some()
        );
        assert!(super::texture(&eng, &ctx, &name).is_err());
    }

    #[test]
    fn a_size_out_of_range_is_refused_and_a_nil_one_is_none() {
        for bad in [
            Value::Int(0),
            Value::Int(i64::from(MAX_SIDE) + 1),
            Value::Num(2.5),
        ] {
            let opts = Value::Map(
                vec![(MAX_SIDE_PIXELS.to_string(), bad.clone())]
                    .into_iter()
                    .collect(),
            );
            assert!(
                side_pixels(Some(&opts), MAX_SIDE_PIXELS, MAX_SIDE).is_err(),
                "{bad:?}"
            );
        }
        let left_out = Value::Map(
            vec![(MAX_SIDE_PIXELS.to_string(), Value::Nil)]
                .into_iter()
                .collect(),
        );
        assert_eq!(
            side_pixels(Some(&left_out), MAX_SIDE_PIXELS, MAX_SIDE).unwrap(),
            None
        );
    }
}
