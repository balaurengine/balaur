//! The raster Godot made of an image at import, read back out.
//!
//! Godot rasterises an SVG when it imports it and keeps the result in
//! `.godot/imported/*.ctex`. A texture imported lossless or lossy holds a
//! plain PNG or WebP inside that file, which the engine here reads as it is,
//! so an SVG carries across as exactly the pixels Godot drew and nothing new
//! has to rasterise it.

use std::path::Path;

/// The PNG or WebP bytes Godot imported `relative` as, and the extension
/// they want. `None` when there is no import, or it was compressed for the
/// GPU and holds neither.
pub(crate) fn raster(root: &Path, relative: &str) -> Option<(Vec<u8>, &'static str)> {
    let import = std::fs::read_to_string(root.join(format!("{relative}.import"))).ok()?;
    let document = crate::godot::parse(&import).ok()?;
    let imported = document.first("remap")?.field("path")?.as_str()?;
    let file = imported.strip_prefix("res://").unwrap_or(imported);
    let bytes = std::fs::read(root.join(file)).ok()?;
    embedded(&bytes)
}

/// The first PNG or WebP inside a `GST2` container: the largest mip, since
/// Godot writes them largest first.
fn embedded(bytes: &[u8]) -> Option<(Vec<u8>, &'static str)> {
    if !bytes.starts_with(b"GST2") {
        return None;
    }
    if let Some(at) = find(bytes, b"RIFF") {
        let size = u32::from_le_bytes(bytes.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let webp = bytes.get(at..at + 8 + size)?;
        return webp
            .get(8..12)
            .filter(|tag| *tag == b"WEBP")
            .map(|_| (webp.to_vec(), "webp"));
    }
    let at = find(bytes, b"\x89PNG\r\n\x1a\n")?;
    // The container states the mip's length in the four bytes before it.
    let size = u32::from_le_bytes(bytes.get(at.checked_sub(4)?..at)?.try_into().ok()?) as usize;
    Some((bytes.get(at..at + size)?.to_vec(), "png"))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::embedded;

    /// The head of this game's `mouse-pointer.svg` import: `GST2`, the
    /// texture's header, the mip's length, then a WebP of that length.
    #[test]
    fn a_webp_mip_comes_out_of_its_container_whole() {
        let mut ctex = b"GST2".to_vec();
        ctex.extend([0u8; 44]);
        let webp = [b"RIFF".as_slice(), &4u32.to_le_bytes(), b"WEBP"].concat();
        ctex.extend((webp.len() as u32).to_le_bytes());
        ctex.extend(&webp);
        let (bytes, extension) = embedded(&ctex).expect("a WebP mip");
        assert_eq!(extension, "webp");
        assert_eq!(bytes, webp);
    }

    #[test]
    fn a_file_that_is_not_a_godot_texture_holds_nothing() {
        assert!(embedded(b"\x89PNG\r\n\x1a\n").is_none());
    }
}
