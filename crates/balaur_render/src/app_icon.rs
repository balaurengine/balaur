//! The application icon: what a window, a dock and a task bar show for the
//! running game.
//!
//! Separate from the frame loop because it is the operating system's idea of
//! the app rather than anything the scene draws, and because macOS wants the
//! image composited onto its own rounded plate first.

use balaur_core::App;

use crate::AppIconConfig;

/// Apply a requested dock/application icon (macOS only for now).
pub(crate) fn apply_app_icon(app: &App) {
    let Some(icon) = app.engine.try_resource::<AppIconConfig>() else {
        return;
    };
    let (source, name) = {
        let mut icon = icon.borrow_mut();
        if !icon.changed {
            return;
        }
        icon.changed = false;
        (icon.bytes.clone(), icon.name.clone())
    };
    #[cfg(target_os = "macos")]
    {
        use objc2::AnyThread;
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::{MainThreadMarker, NSData};
        let Some(bytes) = dock_icon_png(&source) else {
            tracing::warn!("app icon not usable: {name}");
            return;
        };
        if let Ok(dump) = std::env::var("BALAUR_ICON_DUMP") {
            let _ = std::fs::write(&dump, &bytes);
        }
        let data = NSData::with_bytes(&bytes);
        let image = NSImage::initWithData(NSImage::alloc(), &data);
        if let (Some(image), Some(mtm)) = (image, MainThreadMarker::new()) {
            let ns_app = NSApplication::sharedApplication(mtm);
            unsafe { ns_app.setApplicationIconImage(Some(&image)) };
            tracing::info!("app icon set from {name}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&source, &name);
    }
}

/// Build a macOS-style dock icon: the source image composited onto a white
/// rounded-rect plate (Big Sur proportions: 824-of-1024 plate, ~185 corner
/// radius) with transparent margins.
#[cfg(target_os = "macos")]
fn dock_icon_png(source: &[u8]) -> Option<Vec<u8>> {
    use image::ImageEncoder;

    const CANVAS: u32 = 1024;
    const PLATE: u32 = 824;
    const RADIUS: f32 = 185.0;
    const LOGO: u32 = 660;
    let source = image::load_from_memory(source).ok()?.to_rgba8();
    let logo =
        image::imageops::resize(&source, LOGO, LOGO, image::imageops::FilterType::CatmullRom);
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
            let mut color = [255u8, 255, 255, 255];
            if x >= logo_min && x < logo_min + LOGO && y >= logo_min && y < logo_min + LOGO {
                let lp = logo.get_pixel(x - logo_min, y - logo_min).0;
                // Alpha-over white.
                let a = f32::from(lp[3]) / 255.0;
                for c in 0..3 {
                    color[c] = (f32::from(lp[c]) * a + 255.0 * (1.0 - a)) as u8;
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
