//! The application icon: what a window, a dock and a task bar show for the
//! running game.
//!
//! Separate from the frame loop because it is the operating system's idea of
//! the app rather than anything the scene draws, and because macOS wants the
//! image composited onto its own rounded plate first.

use balaur_core::App;

use crate::AppIconConfig;

/// What the preparing thread sends back: the image it made, or `None` when
/// the picture would not read, and the path it was asked for.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
type Prepared = (Option<Ready>, String);

/// A dock plate encoded as PNG on macOS, which AppKit takes as data; the
/// pixels themselves on Windows and Linux, which winit takes.
#[cfg(target_os = "macos")]
type Ready = Vec<u8>;
#[cfg(any(target_os = "windows", target_os = "linux"))]
type Ready = image::RgbaImage;

/// The icon a worker thread is still preparing, if one is.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
static PENDING: std::sync::Mutex<Option<std::sync::mpsc::Receiver<Prepared>>> =
    std::sync::Mutex::new(None);

/// How many frames run before the icon is handed to the desktop.
const AFTER_FRAMES: u64 = 2;

/// The side a window icon is handed over at: Windows' largest taskbar size.
#[cfg(any(target_os = "windows", target_os = "linux"))]
const WINDOW_SIDE: u32 = 256;

/// Apply a requested dock, taskbar or window icon: `window::set_app_icon`, or
/// `[application] icon` when no script named one.
///
/// The preparing runs on its own thread: decoding the picture, resizing it
/// and encoding a 1024-square plate took the whole of the first frame, which
/// is the frame a window has nothing else to show. Only the hand-over stays
/// here, on the main thread AppKit and winit insist on.
pub(crate) fn apply_app_icon(
    app: &App,
    window: &mut kiss3d::window::Window,
    on_screen: bool,
    frame: u64,
) {
    // An offscreen run has no dock entry, and a shell still assembling is
    // what somebody is waiting for: handing the plate to AppKit costs about
    // 66 ms on the main thread.
    if !on_screen || frame < AFTER_FRAMES {
        return;
    }
    follow_settings(app, frame);
    let Some(icon) = app.engine.try_resource::<AppIconConfig>() else {
        return;
    };
    let asked = {
        let mut icon = icon.borrow_mut();
        if icon.changed {
            icon.changed = false;
            Some((icon.bytes.clone(), icon.name.clone(), icon.plate))
        } else {
            None
        }
    };
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let Some((ready, name)) = hand_over(asked) else {
            return;
        };
        let Some(ready) = ready else {
            tracing::warn!("app icon not usable: {name}");
            return;
        };
        #[cfg(target_os = "macos")]
        {
            let _ = window;
            show(&ready, &name);
        }
        #[cfg(not(target_os = "macos"))]
        show(window, ready, &name);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (asked, window);
    }
}

/// The settings the icon is read from when no script names one.
const ICON: &str = "application/icon";
const ICON_DARK: &str = "application/icon_dark";

/// Marks an icon read from `[application]`, and which half of the system it
/// was read for; `window::set_app_icon` removes it, and the script's icon stays.
pub(crate) struct SettingsIconState {
    dark: bool,
}

/// Read `[application] icon` on the first frame that hands one over, and again
/// when the system turns dark or light while the icon is still that one.
fn follow_settings(app: &App, frame: u64) {
    let dark = balaur_core::facts::device(&app.engine).dark_mode;
    let read = match app.engine.try_resource::<SettingsIconState>() {
        Some(was) => was.borrow().dark != dark,
        None => frame == AFTER_FRAMES && app.engine.try_resource::<AppIconConfig>().is_none(),
    };
    if read {
        from_settings(app, dark);
    }
}

/// The icon `[application]` names, or its dark form while the system is
/// dark. Not inside a macOS bundle: its asset catalog already carries the
/// dark and tinted forms, which a plate drawn here would cover.
fn from_settings(app: &App, dark: bool) {
    if inside_macos_bundle() {
        return;
    }
    let named = |key: &str| {
        balaur_core::settings::get(&app.engine, key)
            .as_ref()
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .filter(|path| !path.is_empty())
    };
    app.engine.insert_resource(SettingsIconState { dark });
    let Some(path) = dark
        .then(|| named(ICON_DARK))
        .flatten()
        .or_else(|| named(ICON))
    else {
        return;
    };
    let current = app
        .engine
        .try_resource::<AppIconConfig>()
        .map(|icon| icon.borrow().name.clone());
    if current.as_deref() == Some(path.as_str()) {
        return;
    }
    let bytes = match app
        .engine
        .resource::<balaur_core::project::ProjectFiles>()
        .borrow()
        .read(&path)
    {
        Ok(bytes) => bytes,
        Err(why) => {
            tracing::warn!("[application] icon {path}: {why:#}");
            return;
        }
    };
    app.engine.insert_resource(AppIconConfig {
        bytes,
        name: path,
        plate: crate::config::WHITE_PLATE,
        changed: true,
    });
}

fn inside_macos_bundle() -> bool {
    cfg!(target_os = "macos")
        && std::env::current_exe()
            .is_ok_and(|exe| exe.to_string_lossy().contains(".app/Contents/MacOS"))
}

/// Start preparing what was asked, and answer what a thread finished.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn hand_over(asked: Option<(Vec<u8>, String, [u8; 4])>) -> Option<Prepared> {
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((source, name, plate)) = asked {
        // In-process and one-way: pixels from a thread that only resizes an
        // image already in memory, read by nothing simulated.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((prepare(&source, plate), name));
            balaur_core::wake::wake();
        });
        *pending = Some(rx);
    }
    match pending.as_ref().map(std::sync::mpsc::Receiver::try_recv) {
        Some(Ok(done)) => {
            *pending = None;
            Some(done)
        }
        Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
            *pending = None;
            None
        }
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn show(bytes: &[u8], name: &str) {
    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};
    if let Ok(dump) = std::env::var("BALAUR_ICON_DUMP") {
        let _ = std::fs::write(&dump, bytes); // os files: a macOS-only dump
    }
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data);
    if let (Some(image), Some(mtm)) = (image, MainThreadMarker::new()) {
        let ns_app = NSApplication::sharedApplication(mtm);
        unsafe { ns_app.setApplicationIconImage(Some(&image)) };
        tracing::info!("app icon set from {name}");
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn show(window: &mut kiss3d::window::Window, pixels: Ready, name: &str) {
    window.set_icon(pixels);
    tracing::info!("window icon set from {name}");
}

/// The picture, square at `side`: a raster decoded and resized, an SVG drawn
/// at that size rather than scaled up from its own.
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn picture(source: &[u8], side: u32) -> Option<image::RgbaImage> {
    let image = if balaur_core::pixels::is_svg(source) {
        let (width, _) = balaur_core::pixels::size(source, &toml::Table::new()).ok()?;
        balaur_core::pixels::rasterize_svg(source, side as f32 / width.max(1) as f32).ok()?
    } else {
        image::load_from_memory(source).ok()?.to_rgba8()
    };
    Some(if image.width() == side && image.height() == side {
        image
    } else {
        image::imageops::resize(&image, side, side, image::imageops::FilterType::CatmullRom)
    })
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn prepare(source: &[u8], _plate: [u8; 4]) -> Option<Ready> {
    picture(source, WINDOW_SIDE)
}

/// Build a macOS-style dock icon: the source image composited onto a rounded
/// plate of `plate`'s colour (Big Sur proportions: 824-of-1024 plate, ~185
/// corner radius) with transparent margins.
#[cfg(target_os = "macos")]
fn prepare(source: &[u8], plate: [u8; 4]) -> Option<Ready> {
    use image::ImageEncoder;

    const CANVAS: u32 = 1024;
    const PLATE: u32 = 824;
    const RADIUS: f32 = 185.0;
    const LOGO: u32 = 660;
    let logo = picture(source, LOGO)?;
    let mut canvas = image::RgbaImage::new(CANVAS, CANVAS);
    let plate_min = ((CANVAS - PLATE) / 2) as f32;
    let plate_max = plate_min + PLATE as f32;
    let inside_plate = |x: f32, y: f32| -> bool {
        if x < plate_min || x > plate_max || y < plate_min || y > plate_max {
            return false;
        }
        let cx = x.clamp(plate_min + RADIUS, plate_max - RADIUS);
        let cy = y.clamp(plate_min + RADIUS, plate_max - RADIUS);
        (x - cx).powi(2) + (y - cy).powi(2) <= RADIUS * RADIUS
    };
    let logo_min = (CANVAS - LOGO) / 2;
    for (x, y, px) in canvas.enumerate_pixels_mut() {
        if inside_plate(x as f32 + 0.5, y as f32 + 0.5) {
            let mut color = [plate[0], plate[1], plate[2], 255];
            if x >= logo_min && x < logo_min + LOGO && y >= logo_min && y < logo_min + LOGO {
                let lp = logo.get_pixel(x - logo_min, y - logo_min).0;
                // Alpha-over the plate.
                let a = f32::from(lp[3]) / 255.0;
                for c in 0..3 {
                    color[c] = (f32::from(lp[c]) * a + f32::from(plate[c]) * (1.0 - a)) as u8;
                }
            }
            *px = image::Rgba(color);
        }
    }
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    encoder
        .write_image(&canvas, CANVAS, CANVAS, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}
