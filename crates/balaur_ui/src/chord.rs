//! Key chords as `[input.actions]` spells them: `cmd+shift+KeyS`, `F5`. One
//! reader for a widget's `shortcut` and `submit_key`, and the actions'.

/// A chord as a scene writes it, through the reader `[input.actions]` uses:
/// modifiers and a W3C key code joined by `+`, as `cmd+shift+KeyS` or `F5`.
/// `cmd` is Command on a Mac and Control elsewhere. A word that names neither
/// a modifier nor a key answers `None`, so a typo is a shortcut that never
/// fires rather than a key nobody asked for.
pub(crate) fn chord(text: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let parts = balaur_input::chord_parts(text).ok()?;
    let mut modifiers = egui::Modifiers::NONE;
    for (held, modifier) in [
        (parts.cmd, egui::Modifiers::COMMAND),
        (parts.ctrl, egui::Modifiers::CTRL),
        (parts.alt, egui::Modifiers::ALT),
        (parts.shift, egui::Modifiers::SHIFT),
    ] {
        if held {
            modifiers |= modifier;
        }
    }
    // egui reads a key by its W3C code as well as by its own name.
    Some((modifiers, egui::Key::from_name(parts.key)?))
}

/// A chord in the spelling the platform shows: `⌘⇧S` on a Mac, `Ctrl+Shift+S`
/// elsewhere. What a menu row draws against its far edge when it says a
/// shortcut and no `trailing` of its own.
pub(crate) fn chord_shown(ctx: &egui::Context, text: &str) -> Option<String> {
    let (modifiers, key) = chord(text)?;
    Some(ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
}
