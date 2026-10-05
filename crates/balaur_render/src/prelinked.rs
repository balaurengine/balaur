//! Shaders linked at export, for a build with no WESL linker.
//!
//! Every place that links a shader goes through [`cached`] with a key made
//! from what it links. With the `compile` feature the link runs, and an
//! export records each result; without it the result comes from the table
//! the export wrote into the pack at [`PRELINKED`].

use std::collections::BTreeMap;
use std::sync::{Mutex, RwLock};

use anyhow::{Context as _, Result, anyhow};
use balaur_core::digest::Hasher;

use crate::material::{Field, FieldType};

/// Where an export writes the table, deflated TOML, inside the pack.
pub const PRELINKED: &str = ".balaur/shaders.toml.deflate";

/// What a link produces that drawing reads: the WGSL, a material's `Params`
/// fields, and whether its shader kept the probe binding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Linked {
    pub wgsl: String,
    pub fields: Vec<Field>,
    pub probes: bool,
}

impl Linked {
    /// An engine shader's link: WGSL and nothing a material reads.
    #[must_use]
    pub fn wgsl(wgsl: String) -> Self {
        Self {
            wgsl,
            ..Self::default()
        }
    }
}

/// The pack's table, in a build with no linker.
static TABLE: RwLock<BTreeMap<u64, Linked>> = RwLock::new(BTreeMap::new());
/// What an export is collecting, while it does.
static RECORDING: Mutex<Option<BTreeMap<u64, Linked>>> = Mutex::new(None);

/// The key for linking `root` from `modules` with `features`, under `kind`:
/// what a material, its transparent variant and an engine shader are kept
/// apart by.
#[must_use]
pub fn key(kind: &str, modules: &[(&str, &str)], root: &str, features: &[(&str, bool)]) -> u64 {
    let mut h = Hasher::new();
    for part in [kind, root] {
        h.write(part.as_bytes());
        h.write(&[0]);
    }
    for (path, source) in modules {
        h.write(path.as_bytes());
        h.write(&[0]);
        h.write(source.as_bytes());
        h.write(&[0]);
    }
    let mut sorted = features.to_vec();
    sorted.sort_unstable();
    for (name, on) in sorted {
        h.write(name.as_bytes());
        h.write(&[u8::from(on)]);
    }
    h.finish().0
}

/// `link`'s result for `key`: linked here when the build can, read from the
/// pack's table when it cannot.
///
/// # Errors
/// If the link fails, or the table holds nothing for `key`.
pub fn cached(key: u64, link: impl FnOnce() -> Result<Linked>) -> Result<Linked> {
    if cfg!(feature = "compile") {
        let linked = link()?;
        if let Some(table) = RECORDING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            table.insert(key, linked.clone());
        }
        return Ok(linked);
    }
    TABLE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
        .ok_or_else(|| anyhow!("this build has no shader linker, and the export linked no shader for this one"))
}

/// [`cached`] for a link that may find nothing to link, which the table
/// records by leaving the key out.
pub fn cached_optional(key: u64, link: impl FnOnce() -> Option<Linked>) -> Option<Linked> {
    cached(key, || link().context("nothing to link")).ok()
}

/// Read the table an export wrote into a pack, for a build with no linker.
///
/// # Errors
/// If the table is not one [`record`] wrote.
pub fn install(bytes: &[u8]) -> Result<()> {
    use std::io::Read as _;
    let mut text = String::new();
    flate2::read::DeflateDecoder::new(bytes)
        .read_to_string(&mut text)
        .context("the prelinked shaders do not inflate")?;
    let table = parse(&text)?;
    *TABLE.write().unwrap_or_else(|e| e.into_inner()) = table;
    Ok(())
}

/// Run `f`, collecting every shader it links, and answer the table for
/// [`PRELINKED`].
///
/// # Errors
/// If `f` fails.
pub fn record(f: impl FnOnce() -> Result<()>) -> Result<Vec<u8>> {
    *RECORDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(BTreeMap::new());
    let ran = f();
    let table = RECORDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default();
    ran?;
    use std::io::Write as _;
    let mut deflated =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    deflated.write_all(write(&table).as_bytes())?;
    Ok(deflated.finish()?)
}

/// Link every shader a game in `pack` can ask for, inside [`record`]: the
/// engine's own in each variant, and each of the project's materials in every
/// form a node may draw it.
#[cfg(feature = "compile")]
pub fn link_everything(eng: &balaur_core::Engine, pack: &balaur_core::Pack) {
    use crate::material::{Material3d, compile, compile_on, compile_with, transparent_variant};
    use crate::shaders;

    let warn = |what: &str, linked: Result<()>| {
        if let Err(why) = linked {
            tracing::warn!("{what} did not link for the export: {why:#}");
        }
    };
    warn("the 2D light map", shaders::light_2d().map(drop));
    warn("2D skinning", shaders::skinned_2d().map(drop));
    for morph in [false, true] {
        warn("3D skinning", shaders::skinned_3d(morph).map(drop));
        for vertex_color in [false, true] {
            warn("the prepass", shaders::link_prepass(vertex_color, morph).map(drop));
        }
    }
    for channel in shaders::CHANNELS {
        warn("a channel view", shaders::channel(shaders::CHANNEL, channel).map(drop));
        warn("a channel view", shaders::channel(shaders::CHANNEL_2D, channel).map(drop));
    }
    let text_mask = Material3d {
        shader: "text_mask.wesl".into(),
        ..Material3d::default()
    };
    warn("the text mask", compile(&text_mask, shaders::TEXT_MASK).map(drop));
    let modules = shaders::plugin_modules(eng);
    for finish in crate::vocabulary::words::FINISHES {
        let material = Material3d {
            features: vec![((*finish).to_string(), true)],
            ..Material3d::default()
        };
        warn(finish, compile_with(&material, shaders::FINISH, &modules).map(drop));
    }
    // A material is a document, so the pack keeps it with the scenes.
    for (path, text) in &pack.scenes {
        let is_material = text.parse::<toml::Table>().ok().is_some_and(|t| {
            t.get("type").and_then(toml::Value::as_str) == Some(crate::material::MATERIAL_ASSET_TYPE)
        });
        if !is_material {
            continue;
        }
        let linked = (|| {
            let material = balaur_core::assets::load_typed::<Material3d>(eng, path)?;
            if material.shader.is_empty() {
                return Ok(());
            }
            let source = crate::material::shader_text(eng, path, &material.shader)?;
            // A node of either dimension may draw it, on a device that does or
            // does not morph, and a 3D one in the transparency pass too.
            for morph in [false, true] {
                compile_on(&material, &source, &modules, morph)?;
                let _ = transparent_variant(&material, &source, &modules, morph);
            }
            Ok(())
        })();
        warn(path, linked);
    }
}

fn write(table: &BTreeMap<u64, Linked>) -> String {
    let rows = table
        .iter()
        .map(|(key, linked)| {
            let mut row = toml::map::Map::new();
            row.insert("key".into(), toml::Value::String(format!("{key:016x}")));
            row.insert("wgsl".into(), toml::Value::String(linked.wgsl.clone()));
            row.insert("probes".into(), toml::Value::Boolean(linked.probes));
            let fields = linked
                .fields
                .iter()
                .map(|f| {
                    toml::Value::Array(vec![
                        toml::Value::String(f.name.clone()),
                        toml::Value::String(f.ty.name().into()),
                        toml::Value::Integer(i64::try_from(f.offset).unwrap_or(i64::MAX)),
                    ])
                })
                .collect();
            row.insert("fields".into(), toml::Value::Array(fields));
            toml::Value::Table(row)
        })
        .collect();
    let mut root = toml::map::Map::new();
    root.insert("shader".into(), toml::Value::Array(rows));
    toml::to_string(&toml::Value::Table(root)).unwrap_or_default()
}

fn parse(text: &str) -> Result<BTreeMap<u64, Linked>> {
    let root: toml::Table = text.parse().context("the prelinked shaders do not parse")?;
    let mut table = BTreeMap::new();
    for row in root
        .get("shader")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let key = row
            .get("key")
            .and_then(toml::Value::as_str)
            .and_then(|k| u64::from_str_radix(k, 16).ok())
            .context("a prelinked shader has no key")?;
        let mut fields = Vec::new();
        for field in row
            .get("fields")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let parts = field.as_array().context("a prelinked field is not a list")?;
            let at = |i: usize| parts.get(i).context("a prelinked field is short");
            fields.push(Field {
                name: at(0)?.as_str().unwrap_or_default().to_string(),
                ty: FieldType::named(at(1)?.as_str().unwrap_or_default())
                    .context("a prelinked field has no type a material writes")?,
                offset: usize::try_from(at(2)?.as_integer().unwrap_or_default()).unwrap_or(0),
            });
        }
        table.insert(
            key,
            Linked {
                wgsl: row
                    .get("wgsl")
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                fields,
                probes: row
                    .get("probes")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
            },
        );
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_reads_back_what_it_wrote() {
        let mut table = BTreeMap::new();
        table.insert(
            key("material", &[("package::m", "fn f() {}")], "package::m", &[("lit", true)]),
            Linked {
                wgsl: "@fragment fn fs() {}\n".into(),
                fields: vec![Field {
                    name: "tint".into(),
                    ty: FieldType::Vec4,
                    offset: 16,
                }],
                probes: true,
            },
        );
        assert_eq!(parse(&write(&table)).unwrap(), table);
    }

    #[test]
    fn an_installed_table_holds_what_an_export_recorded() {
        let shader = Linked::wgsl("@compute @workgroup_size(1) fn main() {}\n".into());
        let bytes = record(|| {
            cached(7, || Ok(shader.clone()))?;
            Ok(())
        })
        .unwrap();
        install(&bytes).unwrap();
        assert_eq!(TABLE.read().unwrap().get(&7), Some(&shader));
    }

    #[test]
    fn a_key_does_not_care_what_order_the_features_came_in() {
        let a = key("m", &[], "r", &[("a", true), ("b", false)]);
        let b = key("m", &[], "r", &[("b", false), ("a", true)]);
        assert_eq!(a, b);
        assert_ne!(a, key("m", &[], "r", &[("a", false), ("b", false)]));
        assert_ne!(a, key("transparent", &[], "r", &[("a", true), ("b", false)]));
    }
}
