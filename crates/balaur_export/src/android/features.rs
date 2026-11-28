//! The hardware an Android manifest says the game uses (`[android]
//! features`), so Play offers it to every device it runs on: Play takes a
//! touchscreen as required unless the manifest says otherwise, and a held
//! orientation as the screen's.

use std::fmt::Write as _;

use anyhow::{Result, bail};

const TOUCHSCREEN: &str = "android.hardware.touchscreen";
/// A pointer standing in for touch: a mouse, a trackpad, a Chromebook's.
const FAKETOUCH: &str = "android.hardware.faketouch";
const PORTRAIT: &str = "android.hardware.screen.portrait";
const LANDSCAPE: &str = "android.hardware.screen.landscape";
/// What a bare feature name is short for.
const HARDWARE_PREFIX: &str = "android.hardware.";

/// `xml` with a `uses-feature` before its `<application>` for each feature
/// `required` names, and each the game could be taken to need and does not:
/// touch, and a held orientation's screen. A feature the template already
/// declares is left as it says.
///
/// # Errors
/// When a name is no feature's, or the template has no `<application>`.
pub(super) fn declare(
    xml: &str,
    required: &[String],
    orientation: balaur::project::Orientation,
) -> Result<String> {
    let mut features: Vec<(String, bool)> = Vec::new();
    for name in required {
        let name = feature_name(name)?;
        if xml.contains(&format!("<uses-feature android:name=\"{name}\"")) {
            bail!("[android] features names {name}, which the template declares itself");
        }
        if !features.iter().any(|(named, _)| *named == name) {
            features.push((name, true));
        }
    }
    let touch = features.iter().any(|(name, _)| name == TOUCHSCREEN);
    let mut optional = |name: &str| {
        if !features.iter().any(|(named, _)| named == name) {
            features.push((name.to_string(), false));
        }
    };
    optional(TOUCHSCREEN);
    // A pointer is touch enough for a game that needs no touchscreen.
    if !touch {
        optional(FAKETOUCH);
    }
    match orientation {
        balaur::project::Orientation::Any => {}
        balaur::project::Orientation::Portrait => optional(PORTRAIT),
        balaur::project::Orientation::Landscape => optional(LANDSCAPE),
    }
    let mut lines = String::new();
    for (name, required) in features {
        if xml.contains(&format!("<uses-feature android:name=\"{name}\"")) {
            continue;
        }
        let _ = writeln!(
            lines,
            "  <uses-feature android:name=\"{name}\" android:required=\"{required}\" />"
        );
    }
    super::before_application(xml, &lines)
}

/// A feature as the manifest names it: `touchscreen` is
/// `android.hardware.touchscreen`; a dotted name is taken whole, as a
/// permission's is.
///
/// # Errors
/// When the name is not one Android could declare.
fn feature_name(name: &str) -> Result<String> {
    let shaped = !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    if !shaped {
        bail!(
            "[android] features names {name:?}, which is not an Android feature: touchscreen, \
             or a whole name such as android.hardware.camera"
        );
    }
    Ok(if name.contains('.') {
        name.to_string()
    } else {
        format!("{HARDWARE_PREFIX}{name}")
    })
}

#[cfg(test)]
mod tests {
    use super::super::AndroidConfig;
    use super::super::test_config::{TEMPLATE, named};
    use balaur::project::Orientation;

    #[test]
    fn touch_is_optional_unless_the_game_needs_it() {
        let xml = named().manifest(TEMPLATE).unwrap();
        for line in [
            r#"<uses-feature android:name="android.hardware.touchscreen" android:required="false" />"#,
            r#"<uses-feature android:name="android.hardware.faketouch" android:required="false" />"#,
        ] {
            assert!(xml.contains(line), "{line}: {xml}");
        }
        assert!(xml.find("uses-feature") < xml.find("<application"), "{xml}");

        let touching = AndroidConfig {
            features: vec![
                "touchscreen".into(),
                "android.hardware.touchscreen".into(),
                "amazon.hardware.fire_tv".into(),
            ],
            ..named()
        }
        .manifest(TEMPLATE)
        .unwrap();
        assert!(
            touching.contains(
                r#"<uses-feature android:name="android.hardware.touchscreen" android:required="true" />"#
            ),
            "{touching}"
        );
        assert!(!touching.contains("faketouch"), "{touching}");
        assert!(
            touching.contains(
                r#"<uses-feature android:name="amazon.hardware.fire_tv" android:required="true" />"#
            ),
            "{touching}"
        );
        assert_eq!(
            touching.matches("android.hardware.touchscreen").count(),
            1,
            "{touching}"
        );
        let why = AndroidConfig {
            features: vec!["Touch Screen".into()],
            ..named()
        }
        .manifest(TEMPLATE)
        .unwrap_err()
        .to_string();
        assert!(why.contains("not an Android feature"), "{why}");
    }

    // A held orientation implies the screen's feature, which a landscape TV
    // or Chromebook lacks for portrait: it is declared optional.
    #[test]
    fn a_held_orientation_requires_no_screen_of_its_own() {
        let held = AndroidConfig {
            orientation: Orientation::Portrait,
            ..named()
        }
        .manifest(TEMPLATE)
        .unwrap();
        assert!(
            held.contains(
                r#"<uses-feature android:name="android.hardware.screen.portrait" android:required="false" />"#
            ),
            "{held}"
        );
        let any = named().manifest(TEMPLATE).unwrap();
        assert!(!any.contains("android.hardware.screen."), "{any}");
    }
}
