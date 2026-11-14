//! The Tony McMapface LUT a game template leaves out of its binary: an export
//! carries it in the pack of a game that names the operator, and the template
//! hands it to the renderer before the window opens.

use anyhow::{Context as _, Result};

use crate::vocabulary::words;

/// Where an export writes the LUT, deflated, inside the pack.
pub const TONY_LUT: &str = ".balaur/tony_mcmapface.lut.deflate";

/// Whether a scene, an asset or a compiled script in `pack` names the Tony
/// McMapface operator.
#[must_use]
pub fn named_in(pack: &balaur_core::Pack) -> bool {
    let word = words::TONY_MCMAPFACE.as_bytes();
    let names = |bytes: &[u8]| bytes.windows(word.len()).any(|w| w == word);
    pack.scenes.values().any(|scene| names(scene.as_bytes()))
        || pack.assets.values().any(|asset| names(asset))
        || pack.scripts.values().any(|script| names(script))
}

/// The LUT deflated for [`TONY_LUT`], from a build that embeds it.
///
/// # Errors
/// If the encoder fails.
pub fn deflated() -> Result<Option<Vec<u8>>> {
    #[cfg(all(feature = "window", feature = "compile"))]
    {
        use std::io::Write as _;
        let mut deflated =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        deflated.write_all(kiss3d::post_processing::TONY_MCMAPFACE_LUT)?;
        Ok(Some(deflated.finish()?))
    }
    #[cfg(not(all(feature = "window", feature = "compile")))]
    Ok(None)
}

/// Hand the pack's LUT to the renderer, for a build that leaves it out. A
/// build with no window has nothing to hand it to.
///
/// # Errors
/// If it does not inflate to a whole LUT.
pub fn install(deflated: &[u8]) -> Result<()> {
    use std::io::Read as _;
    let mut lut = Vec::new();
    flate2::read::DeflateDecoder::new(deflated)
        .read_to_end(&mut lut)
        .with_context(|| format!("{TONY_LUT} does not inflate"))?;
    #[cfg(feature = "window")]
    {
        let whole = kiss3d::post_processing::TONY_LUT_BYTES;
        anyhow::ensure!(
            lut.len() == whole,
            "{TONY_LUT} holds {} bytes, where a LUT is {whole}",
            lut.len()
        );
        // A second app in one process finds the LUT handed over already.
        kiss3d::post_processing::set_tony_lut(lut);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> balaur_core::Pack {
        let mut pack = balaur_core::Pack::default();
        pack.scenes
            .insert("main.toml".into(), "[root]\ntype = \"node3d\"\n".into());
        pack.assets
            .insert("sky.png".into(), vec![0x89, b'P', b'N', b'G']);
        pack
    }

    #[test]
    fn a_pack_that_never_names_the_operator_carries_no_lut() {
        assert!(!named_in(&pack()));
    }

    #[test]
    fn a_scene_an_asset_or_a_script_naming_the_operator_each_asks_for_the_lut() {
        let named = format!("tonemap = \"{}\"\n", words::TONY_MCMAPFACE);
        let mut scene = pack();
        scene.scenes.insert("lit.toml".into(), named.clone());
        let mut asset = pack();
        asset
            .assets
            .insert("sun.environment".into(), named.clone().into_bytes());
        let mut script = pack();
        script
            .scripts
            .insert("scripts/look.bs".into(), named.into_bytes());
        assert!(named_in(&scene) && named_in(&asset) && named_in(&script));
    }

    #[test]
    fn a_lut_that_does_not_inflate_is_refused() {
        assert!(install(b"not deflate").is_err());
    }
}
