//! Key chords as a scene or a script spells them: `cmd+shift+s`, `f5`. One
//! reader for the `shortcut` property and the `ui::shortcut` binding.

use crate::vocabulary::words as w;

/// A chord as a scene or a script writes it: modifiers and a key joined by
/// `+`, in any case, as in `cmd+shift+s` or `f5`.
///
/// One spelling for the `shortcut` property and the `ui::shortcut` binding,
/// and one place that knows `cmd` is Command on a Mac and Control elsewhere.
/// A word that names neither a modifier nor a key answers `None`, so a typo
/// is a shortcut that never fires rather than a key nobody asked for.
pub(crate) fn chord(text: &str) -> Option<(egui::Modifiers, egui::Key)> {
    let mut modifiers = egui::Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let named = [
            (w::CMD, egui::Modifiers::COMMAND),
            (w::CTRL, egui::Modifiers::CTRL),
            (w::ALT, egui::Modifiers::ALT),
            (w::SHIFT, egui::Modifiers::SHIFT),
        ]
        .into_iter()
        .find(|(name, _)| part.eq_ignore_ascii_case(name));
        if let Some((_, modifier)) = named {
            modifiers |= modifier;
            continue;
        }
        // egui names its keys `Escape`, `F5`, `Backslash`; a scene writes
        // them the way it writes everything else, in lower case.
        key = egui::Key::from_name(part).or_else(|| capitalised(part, egui::Key::from_name));
        key?;
    }
    Some((modifiers, key?))
}

/// `part` in lower case with its first letter raised, the way egui spells a
/// key, handed to `f` from a buffer on the stack. `ui.shortcut` is asked for
/// every chord on every pass, and the copies this made were most of its cost.
fn capitalised<T>(part: &str, f: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    let mut buffer = [0u8; 32];
    let spelled = buffer.get_mut(..part.len())?;
    spelled.copy_from_slice(part.as_bytes());
    spelled.make_ascii_lowercase();
    spelled.first_mut()?.make_ascii_uppercase();
    f(std::str::from_utf8(spelled).ok()?)
}

/// A chord in the spelling the platform shows: `⌘⇧S` on a Mac, `Ctrl+Shift+S`
/// elsewhere. What a menu row draws against its far edge when it says a
/// shortcut and no `trailing` of its own.
pub(crate) fn chord_shown(ctx: &egui::Context, text: &str) -> Option<String> {
    let (modifiers, key) = chord(text)?;
    Some(ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
}
