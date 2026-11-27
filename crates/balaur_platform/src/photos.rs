//! `device.pick_photo` where the picture is a file: the system's file dialog
//! on a desktop (`rfd`), and the media types every picker answers with.

use std::path::Path;

use crate::PlatformEvent;

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

/// The answer for a picture read from `path`, or why it could not be read.
#[must_use]
pub fn picked_file(request: u64, path: &Path) -> PlatformEvent {
    match std::fs::read(path) {
        Ok(bytes) => PlatformEvent::Picked {
            request,
            bytes,
            media_type: media_type(path).to_string(),
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
pub fn pick_from_files(request: u64, report: &std::sync::mpsc::Sender<PlatformEvent>) {
    let asked = rfd::AsyncFileDialog::new()
        .add_filter("Pictures", &PICTURES.map(|(extension, _)| extension))
        .pick_file();
    let report = report.clone();
    let spawned = std::thread::Builder::new()
        .name("balaur-pick-photo".into())
        .spawn(move || {
            let event = match pollster::block_on(asked) {
                Some(file) => picked_file(request, file.path()),
                None => PlatformEvent::Cancelled { request },
            };
            balaur_core::replay::report(&report, event);
        });
    if let Err(err) = spawned {
        tracing::warn!("device: pick_photo: {err}");
    }
}
