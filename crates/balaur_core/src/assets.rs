//! Shared game content: what Godot calls a Resource.
//!
//! An asset is data several nodes want to *share* and that is too big to
//! inline in a scene node — an animation clip, later a prefab or a material.
//! `Engine::resource` is already the engine's typemap of singletons, so
//! content takes the other word (`docs/NAMING.md` D1).
//!
//! **One rule: a string is a reference, a table is a definition.** Every
//! asset-typed component property accepts either, and
//! `crate::components` applies that rule once for every asset type that will
//! ever be registered.
//!
//! Three reference forms an author writes, and no more:
//!
//! | Form | Means |
//! |---|---|
//! | `"animations/hero.toml"` | the whole file |
//! | `"animations/hero.toml#run"` | the entry named `run` inside it |
//! | `"#hero_idle"` | an `[[assets]]` block in the same scene file |
//!
//! A fourth, `"#!<hex>"`, is written by nobody: it is what an inline
//! definition is *rewritten to* once the cache has recorded it, and it takes
//! a `#entry` suffix for the same reason a file does — an inline table is as
//! free to be a library of named assets as a file is.
//!
//! Core never learns what an asset *is*: a plugin registers a parser with
//! `App::register_asset_type`, the parser returns an opaque `Rc<dyn Any>`, and
//! the plugin downcasts it — exactly as the typemap does. Sharing is the
//! default (two nodes naming one path get one parsed object); [`duplicate`]
//! opts out with a private copy.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

use crate::collections::DetHashMap;
use crate::engine::Engine;
use crate::project::ProjectRoot;

/// Parse one asset type's definition table into an object only its plugin
/// understands.
pub type AssetParseFn = Box<dyn Fn(&toml::Value) -> Result<Rc<dyn Any>>>;

/// Asset parsers, appended during plugin build and read-only afterwards —
/// the same shape as `ComponentRegistry`.
#[derive(Default)]
pub struct AssetTypeRegistry(pub Vec<(String, AssetParseFn)>);

impl AssetTypeRegistry {
    pub fn parser(&self, type_name: &str) -> Option<&AssetParseFn> {
        self.0.iter().find(|(n, _)| n == type_name).map(|(_, p)| p)
    }

    /// Registered type names, in registration order, for error messages.
    pub fn names(&self) -> Vec<String> {
        self.0.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// One `[[assets]]` block in a scene document: Godot's `[sub_resource]`.
#[derive(Deserialize, Clone)]
pub struct SceneAsset {
    /// What `#id` refers to, scoped to the scene declaring it.
    pub id: String,
    /// The asset type name some plugin registered.
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(flatten)]
    pub body: toml::map::Map<String, toml::Value>,
}

/// A resolved reference: what the cache is keyed by.
///
/// Resolution is what turns the three textual forms into one of these, so the
/// cache never sees two spellings of one asset.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AssetRef {
    /// A whole file, project-relative.
    File(String),
    /// A named entry inside a file.
    Entry(String, String),
    /// An `[[assets]]` block, in the scene identified by its source digest.
    Scoped(u64, String),
    /// A definition table written inline on a component property, keyed by a
    /// digest of its content so re-applying the same table is idempotent.
    Inline(u64),
    /// A named entry inside an inline definition — the same relationship
    /// [`Self::Entry`] has to [`Self::File`]. An inline table is as free to be
    /// a library of named assets as a file is, and an editor writing one needs
    /// its entries addressable to name a clip.
    InlineEntry(u64, String),
}

impl std::fmt::Display for AssetRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => write!(f, "{path}"),
            Self::Entry(path, entry) => write!(f, "{path}#{entry}"),
            Self::Scoped(_, id) => write!(f, "#{id}"),
            Self::Inline(digest) => write!(f, "#!{digest:016x}"),
            Self::InlineEntry(digest, entry) => write!(f, "#!{digest:016x}#{entry}"),
        }
    }
}

/// A definition and the type name that decides which parser reads it.
#[derive(Clone)]
struct Definition {
    /// From the document's or the block's `type` key. Absent means nothing
    /// said, which is an error at parse time rather than at resolution.
    type_name: Option<String>,
    body: toml::Value,
}

/// The asset cache: one parsed object per resolved reference, so two nodes
/// naming one path share it. Owned by this module; every writer comes through
/// the functions below.
#[derive(Default)]
pub struct AssetState {
    definitions: DetHashMap<AssetRef, Definition>,
    parsed: DetHashMap<AssetRef, Rc<dyn Any>>,
    /// `[[assets]]` blocks by scene source digest, then by id.
    scopes: DetHashMap<u64, DetHashMap<String, Definition>>,
    /// The scene currently being instantiated, if any.
    current_scope: Option<u64>,
}

impl AssetState {
    /// One textual reference as a cache key.
    ///
    /// The error names the reference, because a scene author reading it has
    /// only the string to go on.
    pub fn resolve(&self, reference: &str) -> Result<AssetRef> {
        let text = reference.trim();
        if text.is_empty() {
            return Err(anyhow!("an asset reference cannot be empty"));
        }
        if let Some(rest) = text.strip_prefix("#!") {
            let (hex, entry) = match rest.split_once('#') {
                Some((hex, entry)) => (hex, (!entry.is_empty()).then_some(entry)),
                None => (rest, None),
            };
            let digest = u64::from_str_radix(hex, 16).map_err(|_| {
                anyhow!("asset reference '{reference}' is not a known inline asset")
            })?;
            return Ok(match entry {
                Some(entry) => AssetRef::InlineEntry(digest, entry.to_string()),
                None => AssetRef::Inline(digest),
            });
        }
        if let Some(id) = text.strip_prefix('#') {
            return self.resolve_scoped(reference, id);
        }
        match text.split_once('#') {
            Some((path, entry))
                if !path.is_empty() && !entry.is_empty() && !entry.contains('#') =>
            {
                Ok(AssetRef::Entry(path.to_string(), entry.to_string()))
            }
            Some(_) => Err(anyhow!(
                "asset reference '{reference}' is malformed: write \"file.toml\", \
                 \"file.toml#entry\" or \"#scene_asset_id\""
            )),
            None => Ok(AssetRef::File(text.to_string())),
        }
    }

    fn resolve_scoped(&self, reference: &str, id: &str) -> Result<AssetRef> {
        if id.is_empty() {
            return Err(anyhow!("asset reference '{reference}' names no id"));
        }
        // While a scene is being instantiated its own blocks win. Outside
        // instantiation — a script or a tool asking afterwards — the most
        // recently entered scene is the one meant.
        if let Some(scene) = self.current_scope {
            if self.scopes.get(&scene).is_some_and(|s| s.contains_key(id)) {
                return Ok(AssetRef::Scoped(scene, id.to_string()));
            }
        }
        self.scopes
            .iter()
            .rev()
            .find(|(_, entries)| entries.contains_key(id))
            .map(|(scene, _)| AssetRef::Scoped(*scene, id.to_string()))
            .ok_or_else(|| {
                anyhow!(
                    "asset reference '{reference}' names no [[assets]] block in any loaded scene"
                )
            })
    }
}

fn state(eng: &Engine) -> Result<Rc<RefCell<AssetState>>> {
    eng.try_resource::<AssetState>()
        .ok_or_else(|| anyhow!("the asset cache is missing; this app was not built by App::new"))
}

/// One textual reference as a cache key, against the engine's asset state.
pub fn resolve(eng: &Engine, reference: &str) -> Result<AssetRef> {
    let resolved = state(eng)?.borrow().resolve(reference)?;
    Ok(resolved)
}

/// The raw definition a reference resolves to.
///
/// No parser is involved, so this works for an asset type no plugin has
/// registered — which is what the `assets` script module hands to scripts.
pub fn definition(eng: &Engine, reference: &str) -> Result<toml::Value> {
    let key = resolve(eng, reference)?;
    Ok(cached_definition(eng, &key, reference)?.body)
}

/// The parsed asset behind a reference, shared with every other holder.
pub fn load(eng: &Engine, reference: &str) -> Result<Rc<dyn Any>> {
    let key = resolve(eng, reference)?;
    let state = state(eng)?;
    let hit = state.borrow().parsed.get(&key).map(Rc::clone);
    if let Some(asset) = hit {
        return Ok(asset);
    }
    let asset = parse(eng, &key, reference)?;
    state.borrow_mut().parsed.insert(key, Rc::clone(&asset));
    Ok(asset)
}

/// [`load`], downcast to what the type's parser produces.
pub fn load_typed<T: 'static>(eng: &Engine, reference: &str) -> Result<Rc<T>> {
    load(eng, reference)?.downcast::<T>().map_err(|_| {
        anyhow!(
            "asset '{reference}' is not a {}",
            std::any::type_name::<T>()
        )
    })
}

/// A private copy: parsed fresh, never cached, never shared. Godot's
/// `duplicate()`.
pub fn duplicate(eng: &Engine, reference: &str) -> Result<Rc<dyn Any>> {
    let key = resolve(eng, reference)?;
    parse(eng, &key, reference)
}

/// A private copy of the definition, read past the cache.
pub fn duplicate_definition(eng: &Engine, reference: &str) -> Result<toml::Value> {
    let key = resolve(eng, reference)?;
    Ok(read_definition(eng, &key, reference)?.body)
}

/// Whether a reference resolves to something that is actually there.
pub fn exists(eng: &Engine, reference: &str) -> bool {
    match resolve(eng, reference) {
        Ok(key) => cached_definition(eng, &key, reference).is_ok(),
        Err(_) => false,
    }
}

/// Forget a reference, so the next load re-reads its source.
///
/// Reloading a file also forgets every `file#entry` cut from it, since they
/// all came out of the text that just changed.
pub fn reload(eng: &Engine, reference: &str) -> Result<()> {
    let key = resolve(eng, reference)?;
    let cache = state(eng)?;
    let mut state = cache.borrow_mut();
    // An inline definition has no source to re-read — the cache entry is the
    // only copy of it — so only the parse is dropped.
    if !matches!(key, AssetRef::Inline(_)) {
        state.definitions.shift_remove(&key);
    }
    state.parsed.shift_remove(&key);
    match &key {
        AssetRef::File(path) => {
            let of_file = |k: &AssetRef| matches!(k, AssetRef::Entry(p, _) if p == path);
            state.definitions.retain(|k, _| !of_file(k));
            state.parsed.retain(|k, _| !of_file(k));
        }
        AssetRef::Inline(digest) => {
            let of_table = |k: &AssetRef| matches!(k, AssetRef::InlineEntry(d, _) if d == digest);
            state.definitions.retain(|k, _| !of_table(k));
            state.parsed.retain(|k, _| !of_table(k));
        }
        _ => {}
    }
    Ok(())
}

/// Record a definition table written inline on a component property.
///
/// Keyed by a digest of the content, so applying the same component twice
/// produces one entry rather than a new one every frame.
pub fn define_inline(eng: &Engine, type_name: &str, body: toml::Value) -> Result<AssetRef> {
    let mut hash = digest_bytes(FNV_OFFSET, type_name.as_bytes());
    hash = digest_value(hash, &body);
    let key = AssetRef::Inline(hash);
    let definition = Definition {
        type_name: (!type_name.is_empty()).then(|| type_name.to_string()),
        body,
    };
    state(eng)?
        .borrow_mut()
        .definitions
        .entry(key.clone())
        .or_insert(definition);
    Ok(key)
}

/// Make a scene's `[[assets]]` blocks the scope `#id` resolves against, and
/// return the previous scope for [`leave_scene_scope`] to restore.
///
/// Scoping is by the scene source's digest, so re-instantiating the same
/// scene reuses one scope rather than growing a new one each time.
pub fn enter_scene_scope(eng: &Engine, source: &str, blocks: &[SceneAsset]) -> Result<Option<u64>> {
    let Some(state) = eng.try_resource::<AssetState>() else {
        if blocks.is_empty() {
            return Ok(None);
        }
        return Err(anyhow!(
            "the scene declares [[assets]] but the asset cache is missing"
        ));
    };
    let scene = digest_bytes(FNV_OFFSET, source.as_bytes());
    let mut entries: DetHashMap<String, Definition> = DetHashMap::default();
    for block in blocks {
        if block.id.is_empty() {
            return Err(anyhow!("an [[assets]] block has no id"));
        }
        let definition = Definition {
            type_name: Some(block.type_name.clone()),
            body: toml::Value::Table(block.body.clone()),
        };
        if entries.insert(block.id.clone(), definition).is_some() {
            return Err(anyhow!("two [[assets]] blocks share the id '{}'", block.id));
        }
    }
    let mut state = state.borrow_mut();
    state.scopes.insert(scene, entries);
    Ok(state.current_scope.replace(scene))
}

/// Restore what [`enter_scene_scope`] returned. The blocks stay registered;
/// only which scene `#id` means goes back.
pub fn leave_scene_scope(eng: &Engine, previous: Option<u64>) {
    if let Some(state) = eng.try_resource::<AssetState>() {
        state.borrow_mut().current_scope = previous;
    }
}

/// The definition for a key, reading its source the first time only.
fn cached_definition(eng: &Engine, key: &AssetRef, reference: &str) -> Result<Definition> {
    let state = state(eng)?;
    let hit = state.borrow().definitions.get(key).cloned();
    if let Some(definition) = hit {
        return Ok(definition);
    }
    let definition = read_definition(eng, key, reference)?;
    state
        .borrow_mut()
        .definitions
        .insert(key.clone(), definition.clone());
    Ok(definition)
}

fn read_definition(eng: &Engine, key: &AssetRef, reference: &str) -> Result<Definition> {
    match key {
        AssetRef::File(path) => {
            let document = read_document(eng, path)?;
            Ok(Definition {
                type_name: declared_type(&document),
                body: document,
            })
        }
        AssetRef::Entry(path, entry) => {
            let document = read_document(eng, path)?;
            let body = entry_of(&document, entry).ok_or_else(|| {
                anyhow!("asset reference '{reference}': '{path}' has no entry named '{entry}'")
            })?;
            Ok(Definition {
                type_name: declared_type(body).or_else(|| declared_type(&document)),
                body: body.clone(),
            })
        }
        AssetRef::Scoped(scene, id) => state(eng)?
            .borrow()
            .scopes
            .get(scene)
            .and_then(|entries| entries.get(id))
            .cloned()
            .ok_or_else(|| anyhow!("asset reference '{reference}' is no longer in scope")),
        // An inline definition was written on a component property, so the
        // cache entry is its source: there is nothing else to read it from.
        AssetRef::Inline(_) => state(eng)?
            .borrow()
            .definitions
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("asset reference '{reference}' is not a known inline asset")),
        AssetRef::InlineEntry(digest, entry) => {
            let document = read_definition(eng, &AssetRef::Inline(*digest), reference)?;
            let body = entry_of(&document.body, entry).ok_or_else(|| {
                anyhow!(
                    "asset reference '{reference}': that inline asset has no entry named '{entry}'"
                )
            })?;
            Ok(Definition {
                type_name: declared_type(body).or(document.type_name),
                body: body.clone(),
            })
        }
    }
}

fn declared_type(value: &toml::Value) -> Option<String> {
    value
        .get("type")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
}

/// The entry `path#entry` names.
///
/// Either a top-level table of that name, or one nested a single level down —
/// which is how a library file groups its entries (`[clips.run]` is `#run`)
/// without core having to know the group's name.
fn entry_of<'a>(document: &'a toml::Value, entry: &str) -> Option<&'a toml::Value> {
    if let Some(found) = document.get(entry).filter(|v| v.is_table()) {
        return Some(found);
    }
    document
        .as_table()?
        .values()
        .find_map(|group| group.get(entry).filter(|v| v.is_table()))
}

/// The asset file's text: from the pack in a packed run and from disk
/// otherwise, exactly as scene files resolve.
///
/// Falling back to `ProjectRoot` matters: a Rust-only app and every test runs
/// with no script host at all.
fn read_document(eng: &Engine, path: &str) -> Result<toml::Value> {
    let source = eng
        .script_host()
        .and_then(|host| host.scene_source(path))
        .map_or_else(|| read_from_disk(eng, path), Ok)?;
    toml::from_str(&source).with_context(|| format!("parsing asset file '{path}'"))
}

fn read_from_disk(eng: &Engine, path: &str) -> Result<String> {
    let root = eng
        .try_resource::<ProjectRoot>()
        .map(|r| r.borrow().0.clone())
        .unwrap_or_default();
    std::fs::read_to_string(root.join(path)).with_context(|| format!("reading asset file '{path}'"))
}

fn parse(eng: &Engine, key: &AssetRef, reference: &str) -> Result<Rc<dyn Any>> {
    let definition = cached_definition(eng, key, reference)?;
    let type_name = definition.type_name.ok_or_else(|| {
        anyhow!("asset '{reference}' declares no `type`, so no parser can be chosen for it")
    })?;
    let registry = eng
        .try_resource::<AssetTypeRegistry>()
        .ok_or_else(|| anyhow!("the asset type registry is missing"))?;
    let registry = registry.borrow();
    let parser = registry.parser(&type_name).ok_or_else(|| {
        anyhow!(
            "asset '{reference}' is of type '{type_name}', which no plugin registered (known: {})",
            registry.names().join(", ")
        )
    })?;
    parser(&definition.body).with_context(|| format!("parsing asset '{reference}'"))
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a. Not cryptographic: it keys a cache, and it has to give the same
/// number on every platform, which is why it is written out here rather than
/// taken from a std hasher whose output is explicitly unspecified.
fn digest_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A digest of a TOML value's structure, so an inline definition keys itself.
/// Structural rather than re-encoded, because `toml::to_string` refuses some
/// value orders that parse perfectly well.
fn digest_value(hash: u64, value: &toml::Value) -> u64 {
    let hash = digest_bytes(hash, value.type_str().as_bytes());
    match value {
        toml::Value::String(s) => digest_bytes(hash, s.as_bytes()),
        toml::Value::Integer(i) => digest_bytes(hash, &i.to_le_bytes()),
        toml::Value::Float(f) => digest_bytes(hash, &f.to_bits().to_le_bytes()),
        toml::Value::Boolean(b) => digest_bytes(hash, &[u8::from(*b)]),
        toml::Value::Datetime(d) => digest_bytes(hash, d.to_string().as_bytes()),
        toml::Value::Array(items) => items.iter().fold(hash, digest_value),
        toml::Value::Table(table) => table.iter().fold(hash, |acc, (key, item)| {
            digest_value(digest_bytes(acc, key.as_bytes()), item)
        }),
    }
}
