//! A `--steam` export: Valve's `steam_api` beside the game, which every
//! desktop runtime opens when it finds one, and `[steam] app_id` as the
//! `steam_appid.txt` a `--dev` build starts with outside the Steam client.
//! Any other export ships no Valve file, `[steam]` table or not: the same
//! game goes to the Mac App Store and itch.io.

use std::path::{Path, PathBuf};

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

/// What an export does about Steam: the project's table, whether this is a
/// Steam build, and whether a dev one.
pub(crate) struct Steam {
    pub(crate) config: SteamConfig,
    pub(crate) build: bool,
    pub(crate) dev: bool,
}

impl Steam {
    /// What `opts` asks of Steam; `bundled` is the target when it ships a
    /// bundle (mobile, the web), which no Steam build is.
    pub(crate) fn for_export(
        manifest: &toml::Table,
        opts: &crate::Options<'_>,
        bundled: Option<&str>,
    ) -> Result<Self> {
        if let Some(target) = bundled.filter(|_| opts.steam) {
            anyhow::bail!("--steam builds a desktop game, and {target} is not one");
        }
        Self::from_manifest(manifest, &opts.path, opts.steam, opts.dev)
    }

    fn from_manifest(
        manifest: &toml::Table,
        project: &Path,
        build: bool,
        dev: bool,
    ) -> Result<Self> {
        let config = SteamConfig::from_manifest(manifest, project)?;
        anyhow::ensure!(
            !build || config.app_id != 0,
            "--steam needs [steam] app_id in project.toml: the id Steamworks gave the game"
        );
        anyhow::ensure!(
            !dev || build,
            "--dev writes steam_appid.txt for a --steam build"
        );
        Ok(Self { config, build, dev })
    }
}

const SO: &str = "libsteam_api.so";
const DYLIB: &str = "libsteam_api.dylib";
const DLL: &str = "steam_api64.dll";

/// The `steam_api` a desktop target opens, or None where Valve ships none
/// (Windows on Arm).
#[must_use]
pub fn steam_api_name(target: &str) -> Option<&'static str> {
    let (platform, _) = crate::split_variant(target);
    match platform {
        words::targets::LINUX_X64 | words::targets::LINUX_ARM64 => Some(SO),
        words::targets::MACOS_UNIVERSAL => Some(DYLIB),
        words::targets::WINDOWS_X64 => Some(DLL),
        _ => None,
    }
}

/// Where a release keeps `library` for the runtime `template`: beside it,
/// named for it, since every runtime shares one directory of the cache.
#[must_use]
pub fn steam_api_beside(template: &Path, library: &str) -> PathBuf {
    let own = template.file_name().map_or_else(String::new, |name| {
        let name = name.to_string_lossy();
        name.strip_suffix(".exe").unwrap_or(&name).to_string()
    });
    template.with_file_name(format!("{own}.{library}"))
}

/// The library a runtime's format opens, for an export that names no target.
fn by_format(bytes: &[u8]) -> Option<&'static str> {
    match crate::extensions::suffix_for(bytes)? {
        words::artifacts::SO => Some(SO),
        words::artifacts::DYLIB => Some(DYLIB),
        _ => Some(DLL),
    }
}

/// On a `--steam` export, copy Valve's library from beside `template` to
/// beside `exe`, where the runtime opens it, and a dev build's
/// `steam_appid.txt` into `run_from`.
pub(crate) fn ship_for(
    steam: &Steam,
    target: Option<&str>,
    template: &Path,
    bytes: &[u8],
    exe: &Path,
    run_from: &Path,
) -> Result<()> {
    if !steam.build {
        return Ok(());
    }
    let name = match target {
        Some(target) => steam_api_name(target)
            .with_context(|| format!("Valve ships no steam_api for {target}"))?,
        None => by_format(bytes).context("--steam needs a desktop runtime")?,
    };
    let named = steam_api_beside(template, name);
    let from = [named.clone(), template.with_file_name(name)]
        .into_iter()
        .find(|path| path.is_file())
        .with_context(|| {
            format!(
                "a --steam export copies Valve's {name} from beside {}, as {}: a release ships it \
                 there, and a source build finds it under crates/balaur_steam/sdk/redistributable_bin",
                template.display(),
                named.display()
            )
        })?;
    let to = exe.with_file_name(name);
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
    let path = run_from.join(words::steam::APP_ID_FILE);
    std::fs::write(&path, steam.config.app_id.to_string())
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_desktop_target_names_valves_file_for_it() {
        assert_eq!(steam_api_name("linux-x64"), Some(SO));
        assert_eq!(steam_api_name("linux-arm64-2d"), Some(SO));
        assert_eq!(steam_api_name("macos-universal"), Some(DYLIB));
        assert_eq!(steam_api_name("windows-x64"), Some(DLL));
        assert_eq!(steam_api_name("windows-arm64"), None);
        assert_eq!(steam_api_name("web"), None);
    }

    #[test]
    fn a_runtime_keeps_its_library_under_its_own_name() {
        let dir = Path::new("/r");
        assert_eq!(
            steam_api_beside(&dir.join("balaur-runtime-windows-x64.exe"), DLL),
            dir.join("balaur-runtime-windows-x64.steam_api64.dll")
        );
        assert_eq!(
            steam_api_beside(&dir.join("balaur-runtime-linux-x64-2d"), SO),
            dir.join("balaur-runtime-linux-x64-2d.libsteam_api.so")
        );
    }

    #[test]
    fn the_table_reads_an_app_id() {
        let manifest: toml::Table = toml::from_str("[steam]\napp_id = 480\n").unwrap();
        let steam = SteamConfig::from_manifest(&manifest, Path::new(".")).unwrap();
        assert_eq!(steam.app_id, 480);
        let unknown: toml::Table = toml::from_str("[steam]\nappid = 480\n").unwrap();
        assert!(SteamConfig::from_manifest(&unknown, Path::new(".")).is_err());
    }

    #[test]
    fn a_steam_build_needs_an_app_id_and_a_dev_one_needs_steam() {
        let none = toml::Table::new();
        assert!(Steam::from_manifest(&none, Path::new("."), true, false).is_err());
        assert!(Steam::from_manifest(&none, Path::new("."), false, true).is_err());
        assert!(Steam::from_manifest(&none, Path::new("."), false, false).is_ok());
    }
}
