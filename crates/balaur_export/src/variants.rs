//! Asset variants: one file per target, under one name.
//!
//! `sprites/hero.png` beside `sprites/hero.android.png` is one asset with two
//! bytes. The pack keeps whichever the target answers to, under the canonical
//! name, so nothing downstream — a scene, a script, the runtime — learns that
//! variants exist. A variant no tag selects never enters the pack at all.
//!
//! Export time, not run time: the bytes have to be chosen before they are
//! written, and a phone should not carry a desktop's textures to pick between
//! them. A dev run reads the source tree and so reads the canonical file.

use balaur::Pack;
use balaur::tags::Tags;

/// What a fold did: the names now carrying a variant, and anything about one
/// of them the author should hear.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Folded {
    pub names: Vec<String>,
    pub warnings: Vec<String>,
}

/// Fold every variant this target answers to onto the name it varies, and
/// drop the rest. `declared` is the project's own tags, which name variants
/// as the engine's do.
///
/// An image folded smaller keeps the size it was drawn at in its sidecar, so
/// a sprite over it stays the size the scene meant; see
/// [`balaur::import::keys::SIZE`].
pub(crate) fn apply(pack: &mut Pack, tags: &Tags, declared: &[String]) -> Folded {
    let mut chosen: Vec<(String, String, usize)> = Vec::new();
    let mut drop = Vec::new();
    for path in pack.assets.keys() {
        let Some((canonical, tag)) = split_variant(path, declared) else {
            continue;
        };
        match rank(tags, &tag) {
            // The narrowest tag wins, which is the order `Tags` holds.
            Some(rank) => chosen.push((canonical, path.clone(), rank)),
            None => drop.push(path.clone()),
        }
    }
    chosen.sort_by(|a, b| a.0.cmp(&b.0).then(b.2.cmp(&a.2)));
    let mut taken = Vec::new();
    for (canonical, path, _) in chosen {
        if taken
            .iter()
            .any(|(name, _): &(String, String)| *name == canonical)
        {
            drop.push(path);
            continue;
        }
        taken.push((canonical, path));
    }
    let mut warnings = Vec::new();
    for (canonical, path) in &taken {
        if let Some(bytes) = pack.assets.remove(path) {
            if let Some(why) = remember_drawn_size(pack, canonical, &bytes) {
                warnings.push(why);
            }
            pack.assets.insert(canonical.clone(), bytes);
        }
    }
    for path in &drop {
        pack.assets.remove(path);
    }
    Folded {
        names: taken.into_iter().map(|(canonical, _)| canonical).collect(),
        warnings,
    }
}

/// Write the canonical image's pixel size into its sidecar before the variant
/// takes its place, answering a warning when the two are not the same shape:
/// the copy would draw stretched, which is a resize gone wrong, not a smaller
/// picture. Not an image, or one that will not decode: nothing to record.
fn remember_drawn_size(pack: &mut Pack, canonical: &str, variant: &[u8]) -> Option<String> {
    let drawn = pack
        .assets
        .get(canonical)
        .and_then(|bytes| image_size(bytes))?;
    let sidecar = balaur::import::sidecar_of(canonical);
    let own = pack.scenes.get(&sidecar).map(String::as_str);
    if let Some(text) = crate::textures::record_drawn(own, drawn) {
        pack.scenes.insert(sidecar, text);
    }
    let (vw, vh) = image_size(variant)?;
    // The height the copy should have at its width, to the nearest pixel.
    let expected = (u64::from(drawn.1) * u64::from(vw)).div_ceil(u64::from(drawn.0).max(1));
    (u64::from(vh).abs_diff(expected) > 1).then(|| {
        format!(
            "the {vw}x{vh} variant of {canonical} is not the shape of the {}x{} original, so it \
             will draw stretched",
            drawn.0, drawn.1
        )
    })
}

/// An image's pixel size off its header, or `None` for bytes that are not one.
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// `sprites/hero.android.png` as `sprites/hero.png` and `android`, or `None`
/// when the middle segment is not a tag anything could answer to.
fn split_variant(path: &str, declared: &[String]) -> Option<(String, String)> {
    let (stem, extension) = path.rsplit_once('.')?;
    let (base, tag) = stem.rsplit_once('.')?;
    let known = balaur::tags::ALL.contains(&tag) || declared.iter().any(|own| own == tag);
    known.then(|| (format!("{base}.{extension}"), tag.to_string()))
}

/// How narrow a tag this target holds is, or `None` when it holds none.
fn rank(tags: &Tags, tag: &str) -> Option<usize> {
    tags.0.iter().position(|held| held == tag)
}

#[cfg(test)]
mod tests {
    use super::apply;
    use balaur::Pack;
    use balaur::tags::Tags;

    /// A real PNG of the size asked for, so the fold can read its header.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// A smaller copy in the original's place still measures as the original:
    /// the fold leaves the drawn size beside it, and says nothing when the
    /// two are the same shape.
    #[test]
    fn folding_a_smaller_variant_records_the_size_it_was_drawn_at() {
        let mut pack = Pack::default();
        pack.assets.insert("sprites/hero.png".into(), png(8, 4));
        pack.assets.insert("sprites/hero.web.png".into(), png(4, 2));
        let folded = apply(&mut pack, &Tags::for_target("web"), &[]);
        assert!(folded.warnings.is_empty(), "{:?}", folded.warnings);
        let sidecar: toml::Table = toml::from_str(
            pack.scenes
                .get("sprites/hero.png.import.toml")
                .expect("a sidecar"),
        )
        .unwrap();
        let size: Vec<i64> = sidecar["size"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_integer().unwrap())
            .collect();
        assert_eq!(size, [8, 4]);
        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&png(4, 2)));
    }

    /// A copy of another shape draws stretched, which is a resize gone wrong
    /// rather than a smaller picture; the author hears about it.
    #[test]
    fn a_variant_of_another_shape_is_warned_about() {
        let mut pack = Pack::default();
        pack.assets.insert("sprites/hero.png".into(), png(8, 4));
        pack.assets.insert("sprites/hero.web.png".into(), png(4, 4));
        let folded = apply(&mut pack, &Tags::for_target("web"), &[]);
        assert_eq!(folded.warnings.len(), 1, "{:?}", folded.warnings);
        assert!(folded.warnings[0].contains("4x4") && folded.warnings[0].contains("8x4"));
    }

    fn pack_with(paths: &[&str]) -> Pack {
        let mut pack = Pack::default();
        for (n, path) in paths.iter().enumerate() {
            pack.assets.insert((*path).to_string(), vec![n as u8]);
        }
        pack
    }

    #[test]
    fn the_variant_a_target_answers_to_becomes_the_asset() {
        let mut pack = pack_with(&["sprites/hero.png", "sprites/hero.android.png"]);
        let folded = apply(&mut pack, &Tags::for_target("android"), &[]);

        assert_eq!(folded.names, ["sprites/hero.png"]);
        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&vec![1]));
        assert!(!pack.assets.contains_key("sprites/hero.android.png"));
    }

    /// A phone's texture is not shipped to a desktop, and the desktop's own
    /// file is the one it always was.
    #[test]
    fn a_variant_no_tag_selects_never_ships() {
        let mut pack = pack_with(&["sprites/hero.png", "sprites/hero.android.png"]);
        apply(&mut pack, &Tags::for_target("linux-x64"), &[]);

        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&vec![0]));
        assert_eq!(pack.assets.len(), 1);
    }

    /// Two variants a target answers to are decided the way an override is:
    /// the narrower tag wins, whatever order the files sort in.
    #[test]
    fn the_narrower_tag_wins_between_two_variants() {
        let mut pack = pack_with(&[
            "sprites/hero.png",
            "sprites/hero.mobile.png",
            "sprites/hero.android.png",
        ]);
        apply(&mut pack, &Tags::for_target("android"), &[]);

        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&vec![2]));
        assert_eq!(pack.assets.len(), 1);
    }

    /// A file with a dot in its name is not a variant: only the closed tag
    /// set counts, so `hero.old.png` ships as itself.
    #[test]
    fn a_middle_segment_that_is_not_a_tag_is_left_alone() {
        let mut pack = pack_with(&["sprites/hero.old.png"]);
        apply(&mut pack, &Tags::for_target("android"), &[]);

        assert!(pack.assets.contains_key("sprites/hero.old.png"));
    }

    /// A project's own tag names a variant too, and a build that does not
    /// answer to it drops the file like any other variant.
    #[test]
    fn a_project_s_own_tag_names_a_variant() {
        let declared = ["demo".to_string()];
        let mut pack = pack_with(&["title.png", "title.demo.png"]);
        let mut tags = Tags::for_target("linux-x64");
        tags.push("demo");
        apply(&mut pack, &tags, &declared);
        assert_eq!(pack.assets.get("title.png"), Some(&vec![1]));

        let mut pack = pack_with(&["title.png", "title.demo.png"]);
        apply(&mut pack, &Tags::for_target("linux-x64"), &declared);
        assert_eq!(pack.assets.get("title.png"), Some(&vec![0]));
        assert_eq!(pack.assets.len(), 1);
    }

    /// A variant with no canonical file beside it still ships, under the name
    /// it varies: a game may have only the phone's copy of a thing.
    #[test]
    fn a_variant_without_a_canonical_file_becomes_one() {
        let mut pack = pack_with(&["sprites/pad.android.png"]);
        apply(&mut pack, &Tags::for_target("android"), &[]);

        assert_eq!(pack.assets.get("sprites/pad.png"), Some(&vec![0]));
    }
}
