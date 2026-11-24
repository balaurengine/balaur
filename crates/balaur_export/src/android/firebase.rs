//! `[android] google_services`: the `google-services.json` Firebase hands out,
//! read as Google's Gradle plugin reads it. Its project, its app id, its API
//! key and its sender id become the string resources `FirebaseInitProvider`
//! starts Firebase from, so `firebase-messaging` works with no Gradle.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The file a game names, as far as Firebase's start needs it.
#[derive(serde::Deserialize)]
struct GoogleServices {
    project_info: ProjectInfo,
    client: Vec<Client>,
}

#[derive(serde::Deserialize)]
struct ProjectInfo {
    project_number: String,
    project_id: String,
}

#[derive(serde::Deserialize)]
struct Client {
    client_info: ClientInfo,
    #[serde(default)]
    api_key: Vec<ApiKey>,
}

#[derive(serde::Deserialize)]
struct ClientInfo {
    mobilesdk_app_id: String,
    android_client_info: AndroidClientInfo,
}

#[derive(serde::Deserialize)]
struct AndroidClientInfo {
    package_name: String,
}

#[derive(serde::Deserialize)]
struct ApiKey {
    current_key: String,
}

/// What Firebase starts from, for one app of the project.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Firebase {
    /// `mobilesdk_app_id`: `google_app_id`.
    pub(crate) app_id: String,
    /// The project number: `gcm_defaultSenderId`.
    pub(crate) sender_id: String,
    /// `google_api_key`.
    pub(crate) api_key: String,
    /// `project_id`.
    pub(crate) project_id: String,
}

/// The client in `path` registered for `application_id`, as the Gradle
/// plugin picks it: one file holds every Android app of the project.
///
/// # Errors
/// When the file is missing or is not a `google-services.json`, or holds
/// no client for this application id, or one with no API key.
pub(crate) fn read(path: &Path, application_id: &str) -> Result<Firebase> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "[android] google_services names {}, which does not read: download \
             google-services.json from the Firebase console's project settings",
            path.display()
        )
    })?;
    let file: GoogleServices = serde_json::from_str(&text).with_context(|| {
        format!(
            "[android] google_services: {} is not a google-services.json",
            path.display()
        )
    })?;
    let client = file
        .client
        .into_iter()
        .find(|client| client.client_info.android_client_info.package_name == application_id)
        .with_context(|| {
            format!(
                "[android] google_services: {} has no app for application_id \
                 \"{application_id}\": add it to the Firebase project and download the file again",
                path.display()
            )
        })?;
    let api_key = client
        .api_key
        .into_iter()
        .map(|key| key.current_key)
        .find(|key| !key.is_empty())
        .with_context(|| {
            format!(
                "[android] google_services: {}'s app for \"{application_id}\" carries no API key",
                path.display()
            )
        })?;
    Ok(Firebase {
        app_id: client.client_info.mobilesdk_app_id,
        sender_id: file.project_info.project_number,
        api_key,
        project_id: file.project_info.project_id,
    })
}

/// The string resources Firebase reads, in the game's own `res/`.
pub(crate) fn write(layout: &Path, firebase: &Firebase) -> Result<PathBuf> {
    use std::fmt::Write as _;
    let values = layout
        .join(super::play::RES)
        .join(crate::vocabulary::words::android::VALUES);
    std::fs::create_dir_all(&values)?;
    let mut body = String::new();
    for (name, value) in [
        ("google_app_id", &firebase.app_id),
        ("gcm_defaultSenderId", &firebase.sender_id),
        ("google_api_key", &firebase.api_key),
        ("project_id", &firebase.project_id),
    ] {
        let _ = writeln!(
            body,
            "  <string name=\"{name}\" translatable=\"false\">{}</string>",
            super::escape(value)
        );
    }
    let path = values.join("firebase.xml");
    std::fs::write(
        &path,
        format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n{body}</resources>\n"),
    )?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Firebase, read, write};

    /// A project with two Android apps, as the Firebase console writes it.
    const FILE: &str = r#"{
  "project_info": {
    "project_number": "842049162737",
    "project_id": "tide-game",
    "storage_bucket": "tide-game.firebasestorage.app"
  },
  "client": [
    {
      "client_info": {
        "mobilesdk_app_id": "1:842049162737:android:0f1e2d3c4b5a6978",
        "android_client_info": { "package_name": "com.studio.other" }
      },
      "api_key": [ { "current_key": "AIzaOther" } ]
    },
    {
      "client_info": {
        "mobilesdk_app_id": "1:842049162737:android:a1b2c3d4e5f60718",
        "android_client_info": { "package_name": "com.studio.tide" }
      },
      "oauth_client": [],
      "api_key": [ { "current_key": "AIzaTide" } ],
      "services": { "appinvite_service": { "other_platform_oauth_client": [] } }
    }
  ],
  "configuration_version": "1"
}"#;

    #[test]
    fn the_app_for_this_application_id_is_the_one_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-services.json");
        std::fs::write(&path, FILE).unwrap();
        assert_eq!(
            read(&path, "com.studio.tide").unwrap(),
            Firebase {
                app_id: "1:842049162737:android:a1b2c3d4e5f60718".into(),
                sender_id: "842049162737".into(),
                api_key: "AIzaTide".into(),
                project_id: "tide-game".into(),
            }
        );
    }

    #[test]
    fn a_file_for_another_app_is_refused_naming_the_application_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-services.json");
        std::fs::write(&path, FILE).unwrap();
        let err = format!("{:#}", read(&path, "com.studio.reef").unwrap_err());
        assert!(err.contains("com.studio.reef"), "{err}");
        let err = format!(
            "{:#}",
            read(&dir.path().join("missing.json"), "x").unwrap_err()
        );
        assert!(err.contains("Firebase console"), "{err}");
    }

    #[test]
    fn the_values_land_as_the_string_resources_firebase_starts_from() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google-services.json");
        std::fs::write(&path, FILE).unwrap();
        let firebase = read(&path, "com.studio.tide").unwrap();
        let written = std::fs::read_to_string(write(dir.path(), &firebase).unwrap()).unwrap();
        for line in [
            r#"<string name="google_app_id" translatable="false">1:842049162737:android:a1b2c3d4e5f60718</string>"#,
            r#"<string name="gcm_defaultSenderId" translatable="false">842049162737</string>"#,
            r#"<string name="google_api_key" translatable="false">AIzaTide</string>"#,
            r#"<string name="project_id" translatable="false">tide-game</string>"#,
        ] {
            assert!(written.contains(line), "{written}");
        }
    }
}
