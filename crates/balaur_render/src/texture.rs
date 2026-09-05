//! Reading an image file: its size from the header, and — in a windowed
//! build — the upload, which logs a file it cannot decode and draws the
//! default texture rather than taking the frame down with it.

use anyhow::{Result, anyhow};

/// An image's pixel size, read from its header alone.
///
/// Not `load_from_memory`: that decodes the whole file, and a scene full of
/// sprites pays for every one of them at load.
pub(crate) fn image_size(bytes: &[u8], name: &str) -> Result<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|why| anyhow!("reading the size of {name}: {why}"))?
        .into_dimensions()
        .map_err(|why| anyhow!("reading the size of {name}: {why}"))
}

/// The name an image is uploaded under: its path and when the file was last
/// written.
///
/// kiss3d's `TextureManager` caches by name and never invalidates, so without
/// the stamp an edited PNG keeps drawing the old pixels for the session. The
/// file's own time rather than the asset generation, which is global: saving
/// one image would otherwise re-decode and re-upload every other one. A
/// packed game has no times to read, and its textures are uploaded once.
#[cfg(any(feature = "kiss3d", test))]
pub(crate) fn upload_name(path: &str, stamp: Option<f64>) -> String {
    match stamp {
        Some(seconds) => format!("{path}#{:x}", seconds.to_bits()),
        None => path.to_string(),
    }
}

#[cfg(feature = "kiss3d")]
mod windowed {
    use std::sync::Arc;

    use balaur_core::Engine;
    use kiss3d::resource::{Texture, TextureManager};
    use kiss3d::scene::{SceneNode2d, SceneNode3d};

    /// Give a freshly built 2D node its image; a path that is empty or does
    /// not decode leaves kiss3d's default white texture, which is what a
    /// sprite with no image chosen yet already draws.
    pub(crate) fn attach_texture_2d(eng: &Engine, node: &mut SceneNode2d, path: &str) {
        if let Some(texture) = upload(eng, path) {
            node.set_texture(texture);
        }
    }

    /// The same for a 3D mesh's `texture`.
    pub(crate) fn attach_texture_3d(eng: &Engine, node: &mut SceneNode3d, path: &str) {
        if let Some(texture) = upload(eng, path) {
            node.set_texture(texture);
        }
    }

    /// The uploaded texture, or `None` to leave the node's default one.
    ///
    /// Decoded here rather than through kiss3d's `add_image_from_memory`,
    /// which is an `expect` on content a scene file names.
    fn upload(eng: &Engine, path: &str) -> Option<Arc<Texture>> {
        if path.is_empty() {
            return None;
        }
        let files = eng.resource::<balaur_core::project::ProjectFiles>();
        let name = super::upload_name(path, files.borrow().mtime(path));
        if let Some(cached) = TextureManager::get_global_manager(|tm| tm.get(&name)) {
            return Some(cached);
        }
        // Bytes rather than a path: a packed game carries its textures inside
        // the pack, with nothing beside it on disk.
        let bytes = match files.borrow().read(path) {
            Ok(bytes) => bytes,
            // A frame is not the place to abort: say which asset is missing
            // and keep drawing the rest of the scene.
            Err(err) => {
                tracing::error!("{err:#}");
                return None;
            }
        };
        match image::load_from_memory(&bytes) {
            Ok(image) => Some(TextureManager::get_global_manager(|tm| {
                tm.add_image(image.clone(), &name)
            })),
            Err(why) => {
                tracing::error!("decoding the image {path}: {why}");
                None
            }
        }
    }
}

#[cfg(feature = "kiss3d")]
pub(crate) use windowed::{attach_texture_2d, attach_texture_3d};

#[cfg(test)]
mod tests {
    use super::{image_size, upload_name};

    /// A 1x1 PNG, so the header the size comes from is a real one.
    fn png() -> Vec<u8> {
        const PIXEL: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        PIXEL.to_vec()
    }

    #[test]
    fn an_images_size_comes_off_its_header() {
        assert_eq!(image_size(&png(), "pixel.png").unwrap(), (1, 1));
    }

    #[test]
    fn an_undecodable_image_is_an_error_naming_the_file() {
        let why = image_size(b"not a png at all", "broken.png").unwrap_err();
        assert!(
            format!("{why:#}").contains("broken.png"),
            "the error does not name the file: {why:#}"
        );
    }

    #[test]
    fn an_edited_image_is_uploaded_under_a_new_name() {
        assert_ne!(
            upload_name("art/hero.png", Some(1.0)),
            upload_name("art/hero.png", Some(2.0))
        );
    }

    #[test]
    fn a_file_nobody_touched_keeps_its_name() {
        assert_eq!(
            upload_name("art/hero.png", Some(1.0)),
            upload_name("art/hero.png", Some(1.0)),
        );
    }

    /// A packed game has no modification times, so every texture is uploaded
    /// once and none of them is ever renamed.
    #[test]
    fn a_packed_texture_is_named_by_its_path_alone() {
        assert_eq!(upload_name("art/hero.png", None), "art/hero.png");
    }
}
