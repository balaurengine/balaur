//! Playing one rig's clip on another: the `bone_map` and `skeleton_profile`
//! assets, and the correction that makes a mapped track land right.
//!
//! A clip's tracks name bones. Two rigs built by two artists name them
//! differently and rest them differently, so a clip authored for one poses
//! the other into a shrug. Godot's answer is a `BoneMap` against a
//! `SkeletonProfile`, and this is that: a map from canonical names to the
//! playing rig's node paths, and a profile saying what each canonical bone's
//! rest is, so a key can be read as a turn *away from rest* rather than as an
//! absolute pose.
//!
//! Two corrections, both per bone and both cheap:
//!
//! * **Rotation.** A key `q` authored against the profile's rest `p` means
//!   `p⁻¹ q` — a delta. On a rig resting at `r` that delta is `r p⁻¹ q`.
//! * **Position.** Scaled by how much longer this rig's bone rests than the
//!   profile's, so a clip authored for a short rig does not leave a tall one
//!   with its feet through the floor.
//!
//! Scale and component tracks pass through untouched: neither means anything
//! different on another rig.
//!
//! The humanoid profile is built in rather than shipped as a file, so
//! `retarget` works in a project that has written no assets of its own; a
//! project wanting different names writes a `skeleton_profile` and says so.

use anyhow::{Result, anyhow, bail};
use balaur_core::collections::DetHashMap;
use balaur_core::skeleton::{Bone, quat_from_euler};
use glamx::{Quat, Vec3};
use std::rc::Rc;

/// The asset type a `bone_map` document declares.
pub const BONE_MAP_ASSET_TYPE: &str = "bone_map";

/// The asset type a `skeleton_profile` document declares.
pub const PROFILE_ASSET_TYPE: &str = "skeleton_profile";

/// The profile every `bone_map` uses when it names none: the canonical
/// humanoid, in the spelling Godot's `SkeletonProfileHumanoid` uses, resting
/// at identity. A rig whose rest is not identity is corrected against it,
/// which is the whole point — the names are what a map has to agree on.
pub const HUMANOID: &[&str] = &[
    "Root",
    "Hips",
    "Spine",
    "Chest",
    "UpperChest",
    "Neck",
    "Head",
    "LeftShoulder",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightShoulder",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "LeftToes",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
    "RightToes",
];

/// One canonical bone: what it is called, and how it rests in the profile.
#[derive(Clone, Debug, Default)]
pub struct ProfileBone {
    pub name: String,
    /// The rest rotation a clip authored against this profile was keyed
    /// relative to, as euler radians.
    pub rest_rotation: Vec3,
    /// The rest offset from the parent bone. Its length is what a position
    /// track is rescaled by; zero leaves the track alone.
    pub rest_position: Vec3,
}

/// A canonical skeleton: the names a clip's tracks may use, and their rests.
#[derive(Clone, Debug, Default)]
pub struct SkeletonProfile {
    pub bones: Vec<ProfileBone>,
}

impl SkeletonProfile {
    /// The built-in humanoid, resting at identity.
    #[must_use]
    pub fn humanoid() -> Self {
        Self {
            bones: HUMANOID
                .iter()
                .map(|&name| ProfileBone {
                    name: name.to_string(),
                    ..ProfileBone::default()
                })
                .collect(),
        }
    }

    fn bone(&self, name: &str) -> Option<&ProfileBone> {
        self.bones.iter().find(|bone| bone.name == name)
    }
}

/// Canonical bone names against the paths they take on one rig.
#[derive(Clone, Debug, Default)]
pub struct BoneMap {
    /// Canonical name to node path, relative to the player's root. Ordered,
    /// so listing a map in the editor is the same order every run.
    pub bones: DetHashMap<String, String>,
    /// The profile the canonical names come from; empty means the humanoid.
    pub profile: String,
}

/// A map with its profile resolved, which is what a playhead holds.
#[derive(Clone, Debug)]
pub struct Retarget {
    pub map: Rc<BoneMap>,
    pub profile: Rc<SkeletonProfile>,
}

impl Retarget {
    /// The node path a track targeting `name` should drive on this rig, or
    /// `None` when the map says nothing about it — in which case the track
    /// keeps the path it was authored with, so a clip that half matches
    /// still plays the half that does.
    #[must_use]
    pub fn path(&self, name: &str) -> Option<&str> {
        self.map.bones.get(name).map(String::as_str)
    }

    /// A rotation key, read as a turn away from the profile's rest and
    /// applied to this rig's.
    #[must_use]
    pub fn rotation(&self, name: &str, rest: Option<&Bone>, key: Quat) -> Quat {
        let Some(profile) = self.profile.bone(name) else {
            return key;
        };
        let Some(rest) = rest else { return key };
        let p = quat_from_euler(profile.rest_rotation);
        quat_from_euler(rest.rest_rotation) * p.inverse() * key
    }

    /// A position key, scaled by how much longer this rig's bone rests than
    /// the profile's. A profile bone resting at the origin names no length
    /// and leaves the key alone.
    #[must_use]
    pub fn position(&self, name: &str, rest: Option<&Bone>, key: Vec3) -> Vec3 {
        let Some(profile) = self.profile.bone(name) else {
            return key;
        };
        let Some(rest) = rest else { return key };
        let from = profile.rest_position.length();
        if from <= 1e-6 {
            return key;
        }
        key * (rest.rest_position.length() / from)
    }
}

/// Parse a `bone_map` document.
///
/// ```toml
/// type = "bone_map"
/// profile = "profiles/humanoid.toml"   # optional; the built-in humanoid otherwise
///
/// [bones]
/// Hips = "Armature/Hips"
/// Spine = "Armature/Hips/Spine"
/// ```
///
/// # Errors
/// When `bones` is missing, is not a table, or holds anything but node paths.
pub fn parse_map(value: &toml::Value) -> Result<BoneMap> {
    let profile = value
        .get("profile")
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(table) = value.get("bones").and_then(toml::Value::as_table) else {
        bail!("a bone map needs a `[bones]` table of canonical name to node path");
    };
    let mut bones = DetHashMap::default();
    for (name, path) in table {
        let path = path
            .as_str()
            .ok_or_else(|| anyhow!("`bones.{name}` is {}, not a node path", path.type_str()))?;
        bones.insert(name.clone(), path.to_string());
    }
    Ok(BoneMap { bones, profile })
}

/// Parse a `skeleton_profile` document.
///
/// ```toml
/// type = "skeleton_profile"
///
/// [[bones]]
/// name = "Hips"
/// rest_rotation = [0.0, 0.0, 0.0]     # euler radians, optional
/// rest_position = [0.0, 1.0, 0.0]     # optional; its length scales positions
/// ```
///
/// A document with no `bones` is the built-in humanoid, which is how a
/// project says "the usual names, my rests".
///
/// # Errors
/// When `bones` is not a list of tables, or a bone has no `name`.
pub fn parse_profile(value: &toml::Value) -> Result<SkeletonProfile> {
    let Some(items) = value.get("bones") else {
        return Ok(SkeletonProfile::humanoid());
    };
    let items = items
        .as_array()
        .ok_or_else(|| anyhow!("`bones` is {}, not a list of bones", items.type_str()))?;
    let bones = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let name = item
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| anyhow!("bone {i} needs a `name`"))?;
            Ok(ProfileBone {
                name: name.to_string(),
                rest_rotation: triple(item, "rest_rotation"),
                rest_position: triple(item, "rest_position"),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SkeletonProfile { bones })
}

fn triple(item: &toml::Value, key: &str) -> Vec3 {
    let Some(list) = item.get(key).and_then(toml::Value::as_array) else {
        return Vec3::ZERO;
    };
    let at = |i: usize| {
        list.get(i)
            .and_then(balaur_core::components::as_f64)
            .map_or(0.0, |v| v as f32)
    };
    Vec3::new(at(0), at(1), at(2))
}

/// What a `bone_map` definition table holds, for the generated reference.
pub(crate) const MAP_ASSET_DOC: &str = r#"Lets one rig play another's clips: `[bones]` pairs canonical bone names with node paths on this rig, `profile` names the `skeleton_profile` they come from.

```toml
type = "bone_map"
# profile = "animations/humanoid.toml"   # left out, the built-in humanoid profile
# Used as animation.play(node, clip, { retarget = "maps/hero.toml" })

[bones]
Hips = "Armature/Hips"
Spine = "Armature/Hips/Spine"
Head = "Armature/Hips/Spine/Neck/Head"
```"#;

/// What a `skeleton_profile` definition table holds, for the reference.
pub(crate) const PROFILE_ASSET_DOC: &str = r#"The canonical skeleton a `bone_map` names bones from. Each `[[bones]]` entry has a `name`, a `rest_rotation` and a `rest_position`; no `bones` means the built-in humanoid.

```toml
type = "skeleton_profile"

[[bones]]
name = "Hips"
rest_position = [0.0, 1.0, 0.0]   # its length scales a retargeted position track

[[bones]]
name = "Spine"
rest_rotation = [0.0, 0.0, 0.0]   # euler radians
```"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn retarget(profile: SkeletonProfile, bones: &[(&str, &str)]) -> Retarget {
        let mut map = BoneMap::default();
        for (name, path) in bones {
            map.bones.insert((*name).to_string(), (*path).to_string());
        }
        Retarget {
            map: Rc::new(map),
            profile: Rc::new(profile),
        }
    }

    #[test]
    fn a_map_renames_a_track_and_leaves_what_it_does_not_name() {
        let r = retarget(SkeletonProfile::humanoid(), &[("Hips", "Armature/Hips")]);
        assert_eq!(r.path("Hips"), Some("Armature/Hips"));
        assert_eq!(r.path("Spine"), None);
    }

    #[test]
    fn a_key_against_a_turned_profile_lands_as_a_turn_from_this_rigs_rest() {
        let mut profile = SkeletonProfile::humanoid();
        profile.bones[1].rest_rotation = Vec3::new(0.0, 0.0, 0.5);
        let r = retarget(profile, &[("Hips", "Hips")]);
        let rest = Bone {
            rest_rotation: Vec3::new(0.0, 0.0, 1.25),
            ..Bone::default()
        };
        // A key sitting exactly on the profile's rest is "no turn at all",
        // so it has to come out as this rig's rest and nothing more.
        let key = quat_from_euler(Vec3::new(0.0, 0.0, 0.5));
        let out = r.rotation("Hips", Some(&rest), key);
        let wanted = quat_from_euler(rest.rest_rotation);
        assert!(
            out.abs_diff_eq(wanted, 1e-5),
            "resting key should land on the rig's rest, got {out:?}"
        );
    }

    #[test]
    fn a_position_is_scaled_by_the_ratio_of_rest_lengths() {
        let mut profile = SkeletonProfile::humanoid();
        profile.bones[1].rest_position = Vec3::new(0.0, 1.0, 0.0);
        let r = retarget(profile, &[("Hips", "Hips")]);
        let rest = Bone {
            rest_position: Vec3::new(0.0, 2.0, 0.0),
            ..Bone::default()
        };
        let out = r.position("Hips", Some(&rest), Vec3::new(0.0, 0.5, 0.0));
        assert!((out.y - 1.0).abs() < 1e-5, "got {out:?}");
        // A bone the profile does not know is left exactly as authored.
        let same = r.position("Nose", Some(&rest), Vec3::new(0.0, 0.5, 0.0));
        assert!((same.y - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_profile_with_no_bones_is_the_humanoid() {
        let doc: toml::Value = toml::from_str("type = \"skeleton_profile\"").unwrap();
        assert_eq!(parse_profile(&doc).unwrap().bones.len(), HUMANOID.len());
    }

    #[test]
    fn a_map_without_bones_says_so() {
        let doc: toml::Value = toml::from_str("type = \"bone_map\"").unwrap();
        assert!(parse_map(&doc).unwrap_err().to_string().contains("[bones]"));
    }
}
