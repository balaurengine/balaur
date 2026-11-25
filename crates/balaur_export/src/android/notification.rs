//! `[android] notification_icon`: the status bar's small icon, as the
//! manifest names it to the shim that posts a local notification
//! (`Alarm.java`) and to FCM, which posts a push that lands in the
//! background itself.

use std::fmt::Write as _;

use anyhow::{Result, bail};

/// The drawable `icon::write_notification_icon` writes at each density.
pub(crate) const DRAWABLE: &str = "notification_icon";
/// The meta-data `Alarm.java` reads its small icon from.
const SHIM: &str = "org.balaur.platform.notification_icon";
/// The meta-data FCM reads for a notification it shows itself.
const FCM: &str = "com.google.firebase.messaging.default_notification_icon";
const APPLICATION_END: &str = "</application>";

/// `xml` with the application's meta-data naming the drawable: the shim's
/// own, and FCM's when the game declares `firebase-messaging`.
pub(super) fn declare(xml: &str, push: bool) -> Result<String> {
    let Some(close) = xml.rfind(APPLICATION_END) else {
        bail!(
            "the template's manifest has no {APPLICATION_END}; scripts/package_runtime.sh writes it"
        );
    };
    let line = xml[..close].rfind('\n').map_or(close, |n| n + 1);
    let mut inside = String::new();
    for name in std::iter::once(SHIM).chain(push.then_some(FCM)) {
        let _ = writeln!(
            inside,
            "    <meta-data android:name=\"{name}\" android:resource=\"@drawable/{DRAWABLE}\" />"
        );
    }
    Ok(format!("{}{inside}{}", &xml[..line], &xml[line..]))
}

#[cfg(test)]
mod tests {
    use super::super::AndroidConfig;
    use super::super::test_config::{TEMPLATE, named};
    use image::{Rgba, RgbaImage};

    /// A project holding `picture` as `art/bell.png`, and a config naming it.
    fn with_icon(picture: &RgbaImage, push: bool) -> (tempfile::TempDir, AndroidConfig) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("art")).unwrap();
        picture.save(dir.path().join("art/bell.png")).unwrap();
        let config = AndroidConfig {
            notification_icon: "art/bell.png".into(),
            capabilities: if push {
                vec![super::super::play::Capability::FirebaseMessaging]
            } else {
                Vec::new()
            },
            project: dir.path().to_path_buf(),
            ..named()
        };
        (dir, config)
    }

    /// A bell's shape: opaque red inside a disc, clear around it.
    fn bell() -> RgbaImage {
        RgbaImage::from_fn(64, 64, |x, y| {
            let (dx, dy) = (x.abs_diff(32), y.abs_diff(32));
            if dx * dx + dy * dy < 20 * 20 {
                Rgba([200, 30, 30, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    #[test]
    fn the_icon_is_a_white_silhouette_at_every_density_the_manifest_names() {
        let (dir, config) = with_icon(&bell(), false);
        let layout = dir.path().join("layout");
        let xml = config.notification_icon(&layout, TEMPLATE).unwrap();
        for (density, side) in [
            ("mdpi", 24),
            ("hdpi", 36),
            ("xhdpi", 48),
            ("xxhdpi", 72),
            ("xxxhdpi", 96),
        ] {
            let path = layout.join(format!("res/drawable-{density}/notification_icon.png"));
            let drawn = image::open(&path).unwrap().to_rgba8();
            assert_eq!(drawn.dimensions(), (side, side), "{density}");
            let middle = drawn.get_pixel(side / 2, side / 2).0;
            assert_eq!(
                middle,
                [255, 255, 255, 255],
                "{density}: white where the bell is"
            );
            assert_eq!(drawn.get_pixel(0, 0).0[3], 0, "{density}: clear around it");
        }
        assert!(
            xml.contains(
                r#"<meta-data android:name="org.balaur.platform.notification_icon" android:resource="@drawable/notification_icon" />"#
            ),
            "{xml}"
        );
        assert!(!xml.contains("firebase"), "{xml}");
        let close = xml.find("</application>").unwrap();
        assert!(xml.find("org.balaur.platform.notification_icon").unwrap() < close);
    }

    // FCM posts a push that lands in the background itself, from its own key.
    #[test]
    fn a_game_with_push_names_the_icon_to_fcm_too() {
        let (dir, config) = with_icon(&bell(), true);
        let xml = config
            .notification_icon(&dir.path().join("layout"), TEMPLATE)
            .unwrap();
        assert!(
            xml.contains(
                r#"<meta-data android:name="com.google.firebase.messaging.default_notification_icon" android:resource="@drawable/notification_icon" />"#
            ),
            "{xml}"
        );
    }

    #[test]
    fn an_opaque_picture_is_refused_since_it_would_draw_a_white_square() {
        let opaque = RgbaImage::from_pixel(64, 64, Rgba([200, 30, 30, 255]));
        let (dir, config) = with_icon(&opaque, false);
        let err = format!(
            "{:#}",
            config
                .notification_icon(&dir.path().join("layout"), TEMPLATE)
                .unwrap_err()
        );
        assert!(err.contains("[android] notification_icon"), "{err}");
        assert!(err.contains("transparen"), "{err}");
    }

    // A game that names none keeps the system's icon and writes nothing.
    #[test]
    fn no_icon_leaves_the_manifest_and_the_layout_alone() {
        let dir = tempfile::tempdir().unwrap();
        let layout = dir.path().join("layout");
        let xml = named().notification_icon(&layout, TEMPLATE).unwrap();
        assert_eq!(xml, TEMPLATE);
        assert!(!layout.exists());
    }
}
