//! The scene key registry indexed by key, which a node's own keys look up.

use super::SceneKeyHandler;
use crate::collections::DetHashMap;
use crate::engine::Engine;

/// The registry's handlers with an index of them by key, so a node looks up
/// the keys it has rather than every node asking every handler for one.
pub(crate) struct SceneKeys<'a> {
    handlers: &'a [(String, SceneKeyHandler)],
    by_key: KeyIndex,
}

/// Each scene key's handlers, by their place in [`super::SceneKeyRegistry`].
type KeyIndex = std::rc::Rc<DetHashMap<String, Vec<usize>>>;

impl SceneKeys<'_> {
    /// Whether any handler takes `key`.
    pub(crate) fn has(&self, key: &str) -> bool {
        self.by_key.contains_key(key)
    }

    /// The handlers `table` has a key for, each with its value, in
    /// registration order whatever the table's: the order plugins rely on.
    pub(crate) fn of<'t>(
        &self,
        table: impl IntoIterator<Item = (&'t String, &'t toml::Value)>,
    ) -> Vec<(&str, &SceneKeyHandler, &'t toml::Value)> {
        let mut found: Vec<(usize, &'t toml::Value)> = table
            .into_iter()
            .filter_map(|(key, value)| Some((self.by_key.get(key.as_str())?, value)))
            .flat_map(|(at, value)| at.iter().map(move |&at| (at, value)))
            .collect();
        found.sort_unstable_by_key(|&(at, _)| at);
        found
            .into_iter()
            .map(|(at, value)| {
                let (key, handler) = &self.handlers[at];
                (key.as_str(), handler, value)
            })
            .collect()
    }
}

/// The last index of [`super::SceneKeyRegistry`] built, and how many handlers it
/// covered. Plugins only add handlers, so a registry of another length is one
/// a plugin has added to since.
#[derive(Default)]
struct SceneKeyIndex(Option<(usize, KeyIndex)>);

pub(super) fn scene_keys<'a>(
    eng: &Engine,
    handlers: &'a [(String, SceneKeyHandler)],
) -> SceneKeys<'a> {
    let cache = eng.try_resource::<SceneKeyIndex>().unwrap_or_else(|| {
        eng.insert_resource(SceneKeyIndex::default());
        eng.resource::<SceneKeyIndex>()
    });
    let mut cache = cache.borrow_mut();
    if let Some((len, by_key)) = &cache.0
        && *len == handlers.len()
    {
        return SceneKeys {
            handlers,
            by_key: by_key.clone(),
        };
    }
    let mut by_key: DetHashMap<String, Vec<usize>> = DetHashMap::default();
    for (at, (key, _)) in handlers.iter().enumerate() {
        by_key.entry(key.clone()).or_default().push(at);
    }
    let by_key = std::rc::Rc::new(by_key);
    cache.0 = Some((handlers.len(), by_key.clone()));
    SceneKeys { handlers, by_key }
}
