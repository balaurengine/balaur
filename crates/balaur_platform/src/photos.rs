//! `device.pick_photo` where the picture is a file: the system's file dialog
//! on a desktop (`rfd`), and the media types every picker answers with.

use std::path::Path;

use crate::PlatformEvent;
use crate::vocabulary::keys;

/// Each picture a file dialog offers, by extension, with its media type:
/// what `image::encode` reads.
const PICTURES: [(&str, &str); 6] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
    ("bmp", "image/bmp"),
];

/// What a file of no picture type is sent as.
const ANY: &str = "application/octet-stream";

/// A picture's media type by its file's extension, or a byte stream's.
#[must_use]
pub fn media_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    PICTURES
        .iter()
        .find(|(known, _)| *known == extension)
        .map_or(ANY, |(_, media)| media)
}

/// The answer for a picture of `media_type` read from `path`, or why it
/// could not be read.
#[must_use]
pub fn picked_file(request: u64, path: &Path, media_type: &str) -> PlatformEvent {
    match std::fs::read(path) {
        Ok(bytes) => PlatformEvent::Picked {
            request,
            bytes,
            media_type: media_type.to_string(),
        },
        Err(err) => PlatformEvent::Failed {
            request,
            message: format!("reading {}: {err}", path.display()),
        },
    }
}

/// The system's file dialog for one picture. Built on the calling thread,
/// which on a Mac is the main one AppKit opens a sheet from, and waited for
/// on a thread of its own, so the frame goes on while the player looks.
#[cfg(not(any(target_family = "wasm", target_os = "ios", target_os = "android")))]
pub(crate) fn pick_from_files(request: u64, report: &std::sync::mpsc::Sender<PlatformEvent>) {
    let asked = rfd::AsyncFileDialog::new()
        .add_filter("Pictures", &PICTURES.map(|(extension, _)| extension))
        .pick_file();
    let report = report.clone();
    let spawned = std::thread::Builder::new()
        .name("balaur-pick-photo".into())
        .spawn(move || {
            let event = match pollster::block_on(asked) {
                Some(file) => picked_file(request, file.path(), media_type(file.path())),
                None => PlatformEvent::Cancelled { request },
            };
            balaur_core::replay::report(&report, event);
        });
    if let Err(err) = spawned {
        tracing::warn!("device: pick_photo: {err}");
    }
}

impl PlatformEvent {
    /// A map shaped as its waiter receives it, from a stand-in or a shim: a
    /// `picked` answer's `bytes` list is bytes, as the OS's picker answers.
    #[must_use]
    pub fn from_answer(request: u64, answer: serde_json::Value) -> Self {
        let picked = answer.get(crate::KIND).and_then(serde_json::Value::as_str)
            == Some(crate::kind::PICKED);
        let bytes = answer
            .get(keys::BYTES)
            .and_then(serde_json::Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_u64().and_then(|byte| u8::try_from(byte).ok()))
                    .collect::<Option<Vec<u8>>>()
            });
        match bytes.filter(|_| picked) {
            Some(bytes) => Self::Picked {
                request,
                bytes,
                media_type: answer
                    .get(keys::TYPE)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            None => Self::Answer { request, answer },
        }
    }
}
