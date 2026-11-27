//! The template's device shims (`crates/balaur_platform/java`), as the export
//! names them for one game.

use anyhow::Result;

/// The attribute naming the template's `org.balaur.platform.Files` provider,
/// which a share hands its file's content URI from.
const FILES_AUTHORITY: &str = "android:authorities";
/// What follows the application id in it: `Files.java` asks for the same.
const FILES_SUFFIX: &str = ".balaur.files";

/// `xml` with the file provider's authority named after `id`: two games on
/// one device cannot share one. A template that predates it is left alone.
pub(super) fn name_files(xml: &str, id: &str) -> Result<String> {
    if super::attr(xml, FILES_AUTHORITY).is_err() {
        return Ok(xml.to_string());
    }
    super::set_attr(xml, FILES_AUTHORITY, &format!("{id}{FILES_SUFFIX}"))
}

#[cfg(test)]
mod tests {
    use super::super::AndroidConfig;
    use super::super::test_config::named;

    /// The manifest scripts/package_runtime.sh writes, read from the script.
    fn staged_manifest() -> String {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/package_runtime.sh");
        let text = std::fs::read_to_string(script).unwrap();
        let start = text.find("<<'MANIFEST'\n").unwrap() + "<<'MANIFEST'\n".len();
        let end = start + text[start..].find("\nMANIFEST\n").unwrap();
        text[start..end].to_string()
    }

    // The device shims' receivers, activities and provider, declared once
    // each, and the provider's authority the game's own.
    #[test]
    fn the_staged_manifest_exports_the_device_shims_under_the_games_name() {
        let config = AndroidConfig {
            permissions: vec!["INTERNET".into(), "POST_NOTIFICATIONS".into()],
            ..named()
        };
        let xml = config.manifest(&staged_manifest()).unwrap();
        for component in [
            r#"<receiver
        android:name="org.balaur.platform.Alarm""#,
            r#"<receiver
        android:name="org.balaur.platform.Chosen""#,
            r#"android:name="org.balaur.platform.Opened""#,
            r#"android:name="org.balaur.platform.Permission""#,
            r#"android:name="org.balaur.platform.Share""#,
            r#"android:name="org.balaur.platform.PickPhoto""#,
            r#"android:name="org.balaur.platform.Files""#,
        ] {
            assert_eq!(xml.matches(component).count(), 1, "{component}\n{xml}");
        }
        assert!(
            xml.contains(r#"android:authorities="com.studio.tide.balaur.files""#),
            "{xml}"
        );
        assert!(!xml.contains("org.balaur.template"), "{xml}");
        for permission in [
            "android.permission.VIBRATE",
            "android.permission.POST_NOTIFICATIONS",
            "android.permission.INTERNET",
        ] {
            let line = format!(r#"<uses-permission android:name="{permission}" />"#);
            assert_eq!(xml.matches(&line).count(), 1, "{permission}\n{xml}");
        }
    }

    // A tap or a link reaches the running game as a new intent: the launcher
    // activity is Game, single-top, and the orientation lands on it.
    #[test]
    fn the_staged_game_activity_is_single_top_and_the_launcher() {
        let config = AndroidConfig {
            orientation: balaur::project::Orientation::Portrait,
            ..named()
        };
        let xml = config.manifest(&staged_manifest()).unwrap();
        let game = xml
            .find(
                r#"<activity
        android:screenOrientation="portrait"
        android:name="org.balaur.platform.Game""#,
            )
            .unwrap_or_else(|| panic!("{xml}"));
        let end = game + xml[game..].find("</activity>").unwrap();
        let activity = &xml[game..end];
        for line in [
            r#"android:launchMode="singleTop""#,
            r#"<meta-data android:name="android.app.lib_name" android:value="main" />"#,
            r#"<category android:name="android.intent.category.LAUNCHER" />"#,
        ] {
            assert!(activity.contains(line), "{line}\n{activity}");
        }
        assert!(!xml.contains("android.app.NativeActivity"), "{xml}");
    }
}
