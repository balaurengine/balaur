//! A Steam template's `steam_api`, shipped where the game's executable loads
//! it, and `[steam] app_id` as the `steam_appid.txt` a `--dev` export starts
//! with outside the Steam client.

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::table_of;
use crate::vocabulary::words;

/// The `[steam]` table.
///
/// ```toml
/// [steam]
/// app_id = 480
/// ```
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SteamConfig {
    /// The id Steamworks gave the game; 0 is unset.
    pub app_id: u32,
}

impl SteamConfig {
    pub(crate) fn from_manifest(manifest: &toml::Table, project: &Path) -> Result<Self> {
        table_of(manifest, words::steam::STEAM, project)
    }
}

/// What a desktop export does about Steam: the project's table, and whether
/// the build is a dev one.
pub(crate) struct Steam {
    pub(crate) config: SteamConfig,
    pub(crate) dev: bool,
}

impl Steam {
    pub(crate) fn from_manifest(manifest: &toml::Table, project: &Path, dev: bool) -> Result<Self> {
        let config = SteamConfig::from_manifest(manifest, project)?;
        Ok(Self { config, dev })
    }
}

const STEAM_API: &[u8] = b"steam_api";

/// The `steam_api` file `template` loads, or `None` for one built without
/// `steam`: its format's name for the library, found among its imports.
pub(crate) fn library_for(template: &[u8]) -> Option<String> {
    let (before, after): (&[u8], &[u8]) = match crate::extensions::suffix_for(template)? {
        words::artifacts::SO => (b"lib", b".so"),
        words::artifacts::DYLIB => (b"lib", b".dylib"),
        _ => (b"", b"64.dll"),
    };
    // Joined here, never one literal: the exporter is in the plain template,
    // and a whole name in its data would read as an import.
    let name = [before, STEAM_API, after].concat();
    let linked = template.windows(name.len()).any(|bytes| bytes == name);
    linked.then(|| String::from_utf8(name).ok()).flatten()
}

/// Ship the `steam_api` `template` loads beside `exe`, which is where its
/// loader looks, and a dev build's `steam_appid.txt` in `run_from`.
pub(crate) fn ship_for(
    steam: &Steam,
    template: &Path,
    bytes: &[u8],
    exe: &Path,
    run_from: &Path,
) -> Result<()> {
    let Some(name) = library_for(bytes) else {
        if steam.config.app_id != 0 {
            tracing::warn!(
                "[steam] app_id is set, but {} was built without steam, so this game never \
                 sees the Steam client: export onto a Steam template",
                template.display()
            );
        }
        return Ok(());
    };
    let from = template.with_file_name(&name);
    anyhow::ensure!(
        from.is_file(),
        "{} loads {name}, which is not beside it: a Steam template ships with Valve's library \
         (VARIANT=steam scripts/package.sh)",
        template.display()
    );
    let to = exe.with_file_name(&name);
    // An export beside its template would copy the library onto itself.
    if to.canonicalize().ok() != from.canonicalize().ok() {
        std::fs::copy(&from, &to)
            .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    }
    tracing::info!("shipped {name} -> {}", to.display());
    write_app_id(steam, run_from)
}

/// The app id Steam reads from the working directory when no client started
/// the game; a release leaves it out, since the client names the app.
fn write_app_id(steam: &Steam, run_from: &Path) -> Result<()> {
    if !steam.dev {
        return Ok(());
    }
    if steam.config.app_id == 0 {
        tracing::warn!("a dev export with no [steam] app_id starts only from the Steam client");
        return Ok(());
    }
    let path = run_from.join(words::steam::APP_ID_FILE);
    std::fs::write(&path, steam.config.app_id.to_string())
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Names joined at run time, as `library_for` joins them: this binary is
    // the one the last test reads.
    fn named(before: &str, after: &str) -> String {
        String::from_utf8([before.as_bytes(), STEAM_API, after.as_bytes()].concat()).unwrap()
    }

    fn template(header: &[u8], import: &str) -> Vec<u8> {
        [header, b"\0code\0", import.as_bytes(), b"\0"].concat()
    }

    #[test]
    fn a_template_names_the_library_its_format_loads() {
        let (so, dylib, dll) = (
            named("lib", ".so"),
            named("lib", ".dylib"),
            named("", "64.dll"),
        );
        let elf = template(b"\x7fELF\x02\x01", &so);
        let mach = template(&[0xca, 0xfe, 0xba, 0xbe], &format!("@loader_path/{dylib}"));
        let pe = template(b"MZ\x90\x00", &dll);
        assert_eq!(library_for(&elf), Some(so.clone()));
        assert_eq!(library_for(&mach), Some(dylib));
        assert_eq!(library_for(&pe), Some(dll));
        assert_eq!(library_for(&template(b"MZ\x90\x00", &so)), None);
        assert_eq!(library_for(b"\x7fELF\x02\x01 no steam here"), None);
    }

    // The exporter is in every plain template, so its own code must not read
    // as a Steam import.
    #[test]
    fn the_exporter_itself_loads_no_steam_api() {
        let own = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(library_for(&own), None);
    }

    #[test]
    fn the_table_reads_an_app_id() {
        let manifest: toml::Table = toml::from_str("[steam]\napp_id = 480\n").unwrap();
        let steam = SteamConfig::from_manifest(&manifest, Path::new(".")).unwrap();
        assert_eq!(steam.app_id, 480);
        let unknown: toml::Table = toml::from_str("[steam]\nappid = 480\n").unwrap();
        assert!(SteamConfig::from_manifest(&unknown, Path::new(".")).is_err());
    }
}
