//! The themes scenes name, by reference.

use super::theme::WidgetTheme;
use balaur_core::Engine;
use std::rc::Rc;

/// The theme in force for a widget: its own, or the nearest ancestor's.
///
/// Resolved once per frame per root rather than per widget, because a screen
/// has one look and walking up the tree for every button to find it out would
/// be work with a known answer.
pub(crate) fn theme_of(
    eng: &Engine,
    reference: &str,
    inherited: &Rc<WidgetTheme>,
) -> Rc<WidgetTheme> {
    if reference.is_empty() {
        return inherited.clone();
    }
    let generation = balaur_core::assets::generation(eng);
    let root = eng.try_resource::<balaur_core::project::ContentRoot>();
    let themes = eng.try_resource::<Themes>().unwrap_or_else(|| {
        eng.insert_resource(Themes::default());
        eng.resource::<Themes>()
    });
    let found = {
        let mut themes = themes.borrow_mut();
        let same_root = match &root {
            Some(root) => themes.root.as_ref() == Some(&root.borrow().0),
            None => themes.root.is_none(),
        };
        if themes.generation != generation || !same_root {
            themes.generation = generation;
            themes.root = root.as_ref().map(|root| root.borrow().0.clone());
            themes.by_reference.clear();
        }
        themes.by_reference.get(reference).cloned()
    };
    if let Some(theme) = found {
        return theme;
    }
    match balaur_core::assets::load_typed::<WidgetTheme>(eng, reference) {
        Ok(theme) => {
            themes
                .borrow_mut()
                .by_reference
                .insert(reference.to_owned(), theme.clone());
            theme
        }
        Err(err) => {
            // A missing theme is a typo in a scene file: said once per reference.
            if balaur_core::logbuf::first_time("widget theme", reference) {
                tracing::warn!("widget theme '{reference}': {err:#}");
            }
            inherited.clone()
        }
    }
}

/// The themes named so far: a pass asks for a root's theme once per widget
/// under it, and an asset's load builds its path each time.
#[derive(Default)]
struct Themes {
    generation: u64,
    root: Option<std::path::PathBuf>,
    by_reference: rustc_hash::FxHashMap<String, Rc<WidgetTheme>>,
}
