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

/// What [`size_of`] has already read, so a caller asking every frame reads
/// the file once.
#[derive(Default)]
pub(crate) struct Sizes {
    generation: u64,
    by_path: balaur_core::collections::DetHashMap<String, (u32, u32)>,
}

/// An image's pixel size, kept until the asset reloads.
///
/// The header is cheap; reaching it is not, because the whole file is read to
/// get at it. Setting a sprite property re-sizes the quad, so this is asked
/// once a frame for every sprite an animation drives.
pub(crate) fn size_of(eng: &crate::Engine, path: &str) -> Result<(u32, u32)> {
    let generation = balaur_core::assets::generation(eng);
    if eng.try_resource::<Sizes>().is_none() {
        eng.insert_resource(Sizes::default());
    }
    let cache = eng.resource::<Sizes>();
    {
        let mut cache = cache.borrow_mut();
        if cache.generation != generation {
            cache.generation = generation;
            cache.by_path.clear();
        }
        if let Some(size) = cache.by_path.get(path) {
            return Ok(*size);
        }
    }
    let bytes = eng
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(path)?;
    let size = image_size(&bytes, path)?;
    cache.borrow_mut().by_path.insert(path.to_string(), size);
    Ok(size)
}

/// The name an image is uploaded under: its path, when the file was last
/// written, and the import settings it was read with.
///
/// kiss3d's `TextureManager` caches by name and never invalidates, so without
/// the stamp an edited PNG keeps drawing the old pixels for the session. The
/// file's own time rather than the asset generation, which is global: saving
/// one image would otherwise re-decode and re-upload every other one. A
/// packed game has no times to read, and its textures are uploaded once.
/// The settings ride in the name too, so changing one image's filter
/// re-uploads that image and leaves every other one alone.
#[cfg(any(feature = "kiss3d", test))]
pub(crate) fn upload_name(path: &str, stamp: Option<f64>, settings: &str) -> String {
    let mut name = match stamp {
        Some(seconds) => format!("{path}#{:x}", seconds.to_bits()),
        None => path.to_string(),
    };
    if !settings.is_empty() {
        name.push('@');
        name.push_str(settings);
    }
    name
}

/// The settings half of an upload name, once this call site has said whether
/// it can blend a premultiplied texture.
///
/// The same image straight is a different upload, so a mesh and a sprite
/// naming one premultiplied file each get their own rather than sharing
/// whichever was drawn first.
#[cfg(any(feature = "kiss3d", test))]
pub(crate) fn upload_stamp(settings: &str, premultiply: bool) -> String {
    if premultiply {
        return settings.to_string();
    }
    format!("{settings}s")
}

#[cfg(feature = "kiss3d")]
mod windowed {
    use std::sync::Arc;

    use balaur_core::Engine;
    use kiss3d::resource::{Texture, TextureManager, TextureSampling, TextureWrapping};
    use kiss3d::scene::{Blend2d, SceneNode2d, SceneNode3d};
    use kiss3d::wgpu;

    /// Give a freshly built 2D node its image; a path that is empty or does
    /// not decode leaves kiss3d's default white texture, which is what a
    /// sprite with no image chosen yet already draws.
    pub(crate) fn attach_texture_2d(eng: &Engine, node: &mut SceneNode2d, path: &str) {
        if let Some(texture) = upload(eng, path, PREMULTIPLY_HONOURED) {
            // A premultiplied texture blended the ordinary way is multiplied
            // by its alpha a second time and comes out dark, so the blend
            // follows the upload rather than the other way round.
            node.set_blend(if texture.premultiplied {
                Blend2d::PremultipliedAlpha
            } else {
                Blend2d::Alpha
            });
            node.set_texture(texture);
        }
    }

    /// The same for a 3D mesh's `texture`.
    pub(crate) fn attach_texture_3d(eng: &Engine, node: &mut SceneNode3d, path: &str) {
        if let Some(texture) = upload(eng, path, PREMULTIPLY_DROPPED) {
            node.set_texture(texture);
        }
    }

    /// Whether a `premultiply = true` reaches the upload.
    ///
    /// A 3D node has no blend mode to match it — kiss3d blends every mesh
    /// straight — so the setting is dropped there rather than drawing the
    /// mesh dark. Named so the two call sites read as the choice they are.
    pub(crate) const PREMULTIPLY_HONOURED: bool = true;
    pub(crate) const PREMULTIPLY_DROPPED: bool = false;

    /// The sampler these settings ask for, in kiss3d's own words, with what
    /// this call site cannot honour taken back out.
    ///
    /// Reported here rather than in `balaur_core`, which resolves the same
    /// settings for a headless run that samples nothing.
    fn sampling(path: &str, resolved: &toml::Table, premultiply_allowed: bool) -> TextureSampling {
        use balaur_core::import::texture::{self, Filter, Wrap};
        let asked = texture::sampling(resolved);
        if texture::anisotropy_refused(&asked) {
            tracing::warn!(
                "{path} asks for anisotropy with a nearest filter, which no GPU samples; \
                 drawing it without"
            );
        }
        if asked.premultiply && !premultiply_allowed {
            tracing::warn!(
                "{path} is premultiplied, which a 3D mesh does not blend; drawing it straight"
            );
        }
        let filter = |which| match which {
            Filter::Nearest => wgpu::FilterMode::Nearest,
            Filter::Linear => wgpu::FilterMode::Linear,
        };
        let wrap = |which| match which {
            Wrap::Repeat => TextureWrapping::Repeat,
            Wrap::Mirror => TextureWrapping::MirroredRepeat,
            Wrap::Clamp => TextureWrapping::ClampToEdge,
        };
        TextureSampling {
            wrap_u: wrap(asked.wrap_u),
            wrap_v: wrap(asked.wrap_v),
            mag_filter: filter(asked.mag),
            min_filter: filter(asked.min),
            mipmap_filter: match asked.mipmap {
                Filter::Nearest => wgpu::MipmapFilterMode::Nearest,
                Filter::Linear => wgpu::MipmapFilterMode::Linear,
            },
            mipmaps: asked.mipmaps,
            anisotropy: asked.anisotropy,
            srgb: asked.srgb,
            premultiply: asked.premultiply && premultiply_allowed,
        }
        // `sane` inside kiss3d lowers an anisotropy the filters refuse; the
        // warning above is what says so.
        .sane()
    }

    /// The uploaded texture, or `None` to leave the node's default one.
    ///
    /// Decoded here rather than through kiss3d's `add_image_from_memory`,
    /// which is an `expect` on content a scene file names.
    pub(crate) fn upload(eng: &Engine, path: &str, premultiply: bool) -> Option<Arc<Texture>> {
        if path.is_empty() {
            return None;
        }
        let files = eng.resource::<balaur_core::project::ProjectFiles>();
        // Resolved rather than read: this runs on every attach, and a sprite
        // is attached on every frame that draws it.
        let settings = balaur_core::import::resolved(eng, path);
        let stamp = super::upload_stamp(&settings.stamp, premultiply);
        let name = super::upload_name(path, files.borrow().mtime(path), &stamp);
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
            Ok(image) => {
                // Built on the miss rather than on every attach, which is
                // also what keeps the two warnings above to one a texture.
                let asked = sampling(path, &settings.settings, premultiply);
                Some(TextureManager::get_global_manager(|tm| {
                    tm.add_image_sampled(image.clone(), &name, asked)
                }))
            }
            Err(why) => {
                tracing::error!("decoding the image {path}: {why}");
                None
            }
        }
    }
}

#[cfg(feature = "kiss3d")]
pub(crate) use windowed::{PREMULTIPLY_DROPPED, attach_texture_2d, attach_texture_3d, upload};

#[cfg(test)]
mod tests {
    use super::{image_size, upload_name, upload_stamp};

    /// The two ways a call site answers `premultiply`, named as they are in
    /// the windowed build, which a headless test does not compile.
    const PREMULTIPLY_HONOURED: bool = true;
    const PREMULTIPLY_DROPPED: bool = false;

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
            upload_name("art/hero.png", Some(1.0), ""),
            upload_name("art/hero.png", Some(2.0), "")
        );
    }

    #[test]
    fn a_file_nobody_touched_keeps_its_name() {
        assert_eq!(
            upload_name("art/hero.png", Some(1.0), ""),
            upload_name("art/hero.png", Some(1.0), ""),
        );
    }

    /// A packed game has no modification times, so every texture is uploaded
    /// once and none of them is ever renamed.
    #[test]
    fn a_packed_texture_is_named_by_its_path_alone() {
        assert_eq!(upload_name("art/hero.png", None, ""), "art/hero.png");
    }

    #[test]
    fn a_texture_read_with_other_settings_is_uploaded_under_another_name() {
        assert_ne!(
            upload_name("art/hero.png", None, ""),
            upload_name("art/hero.png", None, "a1b2")
        );
    }

    /// A 3D mesh has no premultiplied blend to draw with, so it uploads the
    /// same file straight — and under its own name, or whichever node was
    /// built first would decide what the other one draws.
    #[test]
    fn the_same_image_straight_is_a_second_upload() {
        assert_ne!(
            upload_stamp("a1b2", PREMULTIPLY_HONOURED),
            upload_stamp("a1b2", PREMULTIPLY_DROPPED)
        );
        assert_eq!(upload_stamp("a1b2", PREMULTIPLY_HONOURED), "a1b2");
    }
}
