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

/// Fold every variant this target answers to onto the name it varies, and
/// drop the rest.
pub(crate) fn apply(pack: &mut Pack, tags: &Tags) -> Vec<String> {
    let mut chosen: Vec<(String, String, usize)> = Vec::new();
    let mut drop = Vec::new();
    for path in pack.assets.keys() {
        let Some((canonical, tag)) = split_variant(path) else {
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
        if taken.iter().any(|(name, _): &(String, String)| *name == canonical) {
            drop.push(path);
            continue;
        }
        taken.push((canonical, path));
    }
    for (canonical, path) in &taken {
        if let Some(bytes) = pack.assets.remove(path) {
            pack.assets.insert(canonical.clone(), bytes);
        }
    }
    for path in &drop {
        pack.assets.remove(path);
    }
    taken.into_iter().map(|(canonical, _)| canonical).collect()
}

/// `sprites/hero.android.png` as `sprites/hero.png` and `android`, or `None`
/// when the middle segment is not a tag anything could answer to.
fn split_variant(path: &str) -> Option<(String, String)> {
    let (stem, extension) = path.rsplit_once('.')?;
    let (base, tag) = stem.rsplit_once('.')?;
    balaur::tags::ALL
        .iter()
        .find(|known| **known == tag)
        .map(|known| (format!("{base}.{extension}"), (*known).to_string()))
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
        let folded = apply(&mut pack, &Tags::for_target("android"));

        assert_eq!(folded, ["sprites/hero.png"]);
        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&vec![1]));
        assert!(!pack.assets.contains_key("sprites/hero.android.png"));
    }

    /// A phone's texture is not shipped to a desktop, and the desktop's own
    /// file is the one it always was.
    #[test]
    fn a_variant_no_tag_selects_never_ships() {
        let mut pack = pack_with(&["sprites/hero.png", "sprites/hero.android.png"]);
        apply(&mut pack, &Tags::for_target("linux-x64"));

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
        apply(&mut pack, &Tags::for_target("android"));

        assert_eq!(pack.assets.get("sprites/hero.png"), Some(&vec![2]));
        assert_eq!(pack.assets.len(), 1);
    }

    /// A file with a dot in its name is not a variant: only the closed tag
    /// set counts, so `hero.old.png` ships as itself.
    #[test]
    fn a_middle_segment_that_is_not_a_tag_is_left_alone() {
        let mut pack = pack_with(&["sprites/hero.old.png"]);
        apply(&mut pack, &Tags::for_target("android"));

        assert!(pack.assets.contains_key("sprites/hero.old.png"));
    }

    /// A variant with no canonical file beside it still ships, under the name
    /// it varies: a game may have only the phone's copy of a thing.
    #[test]
    fn a_variant_without_a_canonical_file_becomes_one() {
        let mut pack = pack_with(&["sprites/pad.android.png"]);
        apply(&mut pack, &Tags::for_target("android"));

        assert_eq!(pack.assets.get("sprites/pad.png"), Some(&vec![0]));
    }
}
