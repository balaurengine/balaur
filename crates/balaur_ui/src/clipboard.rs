//! The desktop's clipboard, which the window layer under egui does not reach:
//! what a pass copies is written to it, and ⌘V or Ctrl+V reads it back as
//! egui's own paste, its text first and an image where it holds none. A tab's
//! clipboard is the page's, handed in as the page's own paste and drop; a run
//! with no clipboard to open goes without.

/// Where a pasted image is kept until a script writes it out, since the
/// pass that saw it ends before the script that hears of it runs.
pub(crate) type Image = std::sync::Arc<egui::ColorImage>;

#[cfg(desktop)]
mod desktop {
    use std::cell::RefCell;

    thread_local! {
        /// Opened once, on first use: opening it talks to the window server.
        static BOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
    }

    pub(super) fn with<T>(use_it: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
        BOARD.with(|board| {
            let mut board = board.borrow_mut();
            if board.is_none() {
                *board = arboard::Clipboard::new().ok();
            }
            board.as_mut().and_then(use_it)
        })
    }
}

/// Before the widgets draw: a paste chord the window layer said only as a
/// key becomes egui's paste of what the clipboard holds.
pub(crate) fn read_paste(ctx: &egui::Context) {
    #[cfg(desktop)]
    {
        let asked = ctx.input(|i| {
            let chord = i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { key: egui::Key::V, pressed: true, modifiers, .. }
                        if modifiers.command
                )
            });
            let said = i
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
            chord && !said
        });
        if !asked {
            return;
        }
        let pasted = desktop::with(|board| match board.get_text() {
            Ok(text) if !text.is_empty() => Some(egui::Event::Paste(text)),
            _ => board.get_image().ok().map(|image| {
                egui::Event::PasteImage(std::sync::Arc::new(
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.width, image.height],
                        &image.bytes,
                    ),
                ))
            }),
        });
        if let Some(event) = pasted {
            ctx.input_mut(|i| i.events.push(event));
        }
    }
    #[cfg(not(desktop))]
    let _ = ctx;
}

/// After the widgets drew: the text a field or a script copied, onto the
/// clipboard.
pub(crate) fn write_copies(ctx: &egui::Context) {
    #[cfg(desktop)]
    {
        let copied = ctx.output(|o| {
            o.commands.iter().rev().find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
        });
        if let Some(text) = copied {
            desktop::with(|board| board.set_text(text).ok());
        }
    }
    #[cfg(not(desktop))]
    let _ = ctx;
}

/// The image the last pass saw pasted, as PNG bytes.
pub(crate) fn png_of(image: &egui::ColorImage) -> anyhow::Result<Vec<u8>> {
    let [width, height] = image.size;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let mut out = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut out),
        &rgba,
        u32::try_from(width)?,
        u32::try_from(height)?,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}
