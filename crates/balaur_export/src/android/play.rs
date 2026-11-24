//! Google's libraries, as `[android] capabilities` names them: the template
//! carries every one under `play/` (`scripts/play_libraries.py stage`), and an
//! export keeps the ones its capabilities need.
//!
//! What a capability keeps is the template's `play/index.json`: its
//! artifacts' dex files, numbered after the template's own `classes.dex`;
//! their resources, linked with the ids the template's R classes were
//! generated against (`play/ids.txt`, renamed to the game's package); and the
//! manifest entries the libraries' own manifests carry, merged per set of
//! capabilities when the template was built. A game that declares none
//! carries none of it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Where the template keeps the libraries, and an export what it kept.
pub(crate) const PLAY: &str = "play";
const INDEX: &str = "index.json";
/// The library resources an export keeps, one directory each, in link order.
pub(crate) use crate::vocabulary::words::android::RES;
/// The ids the template's R classes hold, under the game's package.
pub(crate) const IDS: &str = "ids.txt";
/// The package `package_runtime.sh` gives the template, which `ids.txt` names.
const TEMPLATE_PACKAGE: &str = "org.balaur.template";
const APPLICATION_ID: &str = "${applicationId}";
/// The meta-data Play Games reads its project id from, through a string
/// resource: a bare number would be read as an integer, which it ignores.
const GAMES_APP_ID: &str = "com.google.android.gms.games.APP_ID";
const GAMES_PROJECT_ID: &str = "game_services_project_id";

/// A Google service the game declares it uses, as a project spells it.
///
/// Closed, as `[apple] capabilities` is: a misspelled one would ship without
/// its library and fail at the player's first call.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Capability {
    /// Play Games Services v2: `platform.*`'s player, achievements,
    /// leaderboards, saved games and server code.
    PlayGames,
    /// Play Billing behind `platform.purchase` and the rest.
    InAppPurchase,
    /// Play's review prompt behind `device.request_review`.
    InAppReview,
}

impl Capability {
    /// The spelling a project writes and the template's index keys.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::PlayGames => "play-games",
            Self::InAppPurchase => "in-app-purchase",
            Self::InAppReview => "in-app-review",
        }
    }
}

/// `play/index.json`, as `play_libraries.py stage` writes it.
#[derive(serde::Deserialize)]
struct Index {
    artifacts: BTreeMap<String, Artifact>,
    capabilities: BTreeMap<String, Needs>,
    manifests: BTreeMap<String, Entries>,
    r: Vec<String>,
}

#[derive(serde::Deserialize)]
struct Artifact {
    dex: Vec<String>,
    res: Option<String>,
}

#[derive(serde::Deserialize)]
struct Needs {
    artifacts: Vec<String>,
    shim: Vec<String>,
}

/// What a set of capabilities adds to the manifest.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Entries {
    pub(crate) permissions: Vec<String>,
    manifest: Vec<String>,
    application: Vec<String>,
}

/// Keep what `capabilities` needs of the layout's `play/` and drop the rest:
/// their dex files beside `classes.dex`, their resources under `play/res`,
/// and the ids those link with. Returns the manifest entries to add.
///
/// # Errors
/// When a capability is declared and the template carries no libraries, or
/// the template's index names a file it does not hold.
pub(crate) fn keep(
    layout: &Path,
    capabilities: &[Capability],
    id: &str,
) -> Result<Option<Entries>> {
    let play = layout.join(PLAY);
    if capabilities.is_empty() {
        if play.exists() {
            std::fs::remove_dir_all(&play).context("dropping the template's Google libraries")?;
        }
        return Ok(None);
    }
    let index_path = play.join(INDEX);
    let text = std::fs::read_to_string(&index_path).with_context(|| {
        format!(
            "[android] capabilities names {}, and this template carries no Google libraries \
             ({} is missing): it predates them, so build or download a newer one",
            names(capabilities),
            index_path.display()
        )
    })?;
    let index: Index =
        serde_json::from_str(&text).context("reading the template's play/index.json")?;
    let mut chosen: Vec<&str> = capabilities.iter().map(|c| c.name()).collect();
    chosen.sort_unstable();
    chosen.dedup();
    let mut artifacts: Vec<&str> = Vec::new();
    let mut dex: Vec<&str> = index.r.iter().map(String::as_str).collect();
    for name in &chosen {
        let needs = index
            .capabilities
            .get(*name)
            .with_context(|| format!("the template's libraries have no {name}"))?;
        artifacts.extend(needs.artifacts.iter().map(String::as_str));
        dex.extend(needs.shim.iter().map(String::as_str));
    }
    // The index's own order, which is dependency order: a library's
    // resources link after those of what it builds on.
    let mut ordered: Vec<(&String, &Artifact)> = index
        .artifacts
        .iter()
        .filter(|(key, _)| artifacts.contains(&key.as_str()))
        .collect();
    ordered.sort_by_key(|(_, artifact)| artifact.dex.first().or(artifact.res.as_ref()).cloned());
    let mut resources = Vec::new();
    for (_, artifact) in &ordered {
        dex.extend(artifact.dex.iter().map(String::as_str));
        resources.extend(artifact.res.iter());
    }
    let staging = layout.join(format!("{PLAY}.kept"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join(RES))?;
    for (n, path) in dex.iter().enumerate() {
        // classes.dex is the template's own shims; these follow it.
        let to = layout.join(format!("classes{}.dex", n + 2));
        std::fs::copy(play.join(path), &to)
            .with_context(|| format!("the template's play/{path} is missing"))?;
    }
    for (n, path) in resources.iter().enumerate() {
        std::fs::rename(play.join(path), staging.join(RES).join(format!("{n:02}")))
            .with_context(|| format!("the template's play/{path} is missing"))?;
    }
    let ids = std::fs::read_to_string(play.join(IDS)).context("the template's play/ids.txt")?;
    let renamed = ids.replace(&format!("{TEMPLATE_PACKAGE}:"), &format!("{id}:"));
    std::fs::write(staging.join(IDS), renamed)?;
    std::fs::remove_dir_all(&play)?;
    std::fs::rename(&staging, &play)?;
    let mut entries = index
        .manifests
        .into_iter()
        .find(|(key, _)| *key == chosen.join(","))
        .map(|(_, entries)| entries)
        .with_context(|| {
            format!(
                "the template's libraries have no manifest for {}",
                chosen.join(",")
            )
        })?;
    for line in entries
        .permissions
        .iter_mut()
        .chain(&mut entries.manifest)
        .chain(&mut entries.application)
    {
        *line = line.replace(APPLICATION_ID, id);
    }
    Ok(Some(entries))
}

/// The manifest with the libraries' entries in it: beside `<application>`,
/// and inside it, with Play Games' project id when it is declared.
pub(crate) fn declare(
    xml: &str,
    entries: &Entries,
    games_project_id: Option<&str>,
) -> Result<String> {
    use std::fmt::Write as _;
    let mut inside = String::new();
    for line in &entries.application {
        let _ = writeln!(inside, "    {line}");
    }
    if games_project_id.is_some() {
        let _ = writeln!(
            inside,
            "    <meta-data android:name=\"{GAMES_APP_ID}\" android:value=\"@string/{GAMES_PROJECT_ID}\" />"
        );
    }
    let mut beside = String::new();
    for line in &entries.manifest {
        let _ = writeln!(beside, "  {line}");
    }
    let Some(open) = xml
        .find("  <application")
        .or_else(|| xml.find("<application"))
    else {
        bail!("the template's manifest has no <application>; scripts/package_runtime.sh writes it");
    };
    let Some(close) = xml.rfind("</application>") else {
        bail!("the template's manifest has no </application>");
    };
    let close = xml[..close].rfind('\n').map_or(close, |n| n + 1);
    Ok(format!(
        "{}{beside}{}{inside}{}",
        &xml[..open],
        &xml[open..close],
        &xml[close..]
    ))
}

/// The string resource Play Games' meta-data names, in the game's own `res/`.
pub(crate) fn write_project_id(layout: &Path, games_project_id: &str) -> Result<PathBuf> {
    let values = layout.join(RES).join("values");
    std::fs::create_dir_all(&values)?;
    let path = values.join("play_games.xml");
    std::fs::write(
        &path,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n  \
             <string name=\"{GAMES_PROJECT_ID}\" translatable=\"false\">{games_project_id}</string>\n\
             </resources>\n"
        ),
    )?;
    Ok(path)
}

/// The kept libraries' resource directories, in link order.
pub(crate) fn resources(layout: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(layout.join(PLAY).join(RES)) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn names(capabilities: &[Capability]) -> String {
    capabilities
        .iter()
        .map(|c| c.name())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{Capability, IDS, PLAY, RES, declare, keep, resources};
    use std::path::Path;

    /// A template's `play/` as `play_libraries.py stage` lays it out: two
    /// capabilities sharing a base library.
    fn template(layout: &Path) {
        let play = layout.join(PLAY);
        for dir in [
            "00-base/res/values",
            "01-games",
            "02-billing",
            "r",
            "shim-play-games",
            "shim-in-app-purchase",
        ] {
            std::fs::create_dir_all(play.join(dir)).unwrap();
        }
        for (path, body) in [
            ("00-base/classes.dex", "base"),
            ("00-base/res/values/values.xml", "<resources/>"),
            ("01-games/classes.dex", "games"),
            ("02-billing/classes.dex", "billing"),
            ("r/classes.dex", "r"),
            ("shim-play-games/classes.dex", "games shim"),
            ("shim-in-app-purchase/classes.dex", "billing shim"),
            (
                IDS,
                "org.balaur.template:integer/google_play_services_version = 0x7f060000\n",
            ),
        ] {
            std::fs::write(play.join(path), body).unwrap();
        }
        let index = serde_json::json!({
            "artifacts": {
                "g:base": { "version": "1", "dex": ["00-base/classes.dex"], "res": "00-base/res" },
                "g:games": { "version": "1", "dex": ["01-games/classes.dex"], "res": null },
                "g:billing": { "version": "1", "dex": ["02-billing/classes.dex"], "res": null },
            },
            "capabilities": {
                "play-games": { "artifacts": ["g:base", "g:games"], "shim": ["shim-play-games/classes.dex"] },
                "in-app-purchase": { "artifacts": ["g:base", "g:billing"], "shim": ["shim-in-app-purchase/classes.dex"] },
            },
            "manifests": {
                "play-games": { "permissions": [], "manifest": ["<queries><package android:name=\"com.google.android.gms\" /></queries>"],
                    "application": ["<provider android:name=\"Init\" android:authorities=\"${applicationId}.init\" />"] },
                "in-app-purchase": { "permissions": ["com.android.vending.BILLING"], "manifest": [], "application": [] },
                "in-app-purchase,play-games": { "permissions": ["com.android.vending.BILLING"], "manifest": [], "application": [] },
            },
            "r": ["r/classes.dex"],
        });
        std::fs::write(play.join("index.json"), index.to_string()).unwrap();
    }

    fn dex(layout: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(layout)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| super::super::is_dex(n))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_game_with_no_capability_ships_no_google_library() {
        let dir = tempfile::tempdir().unwrap();
        template(dir.path());
        assert!(keep(dir.path(), &[], "com.studio.tide").unwrap().is_none());
        assert!(!dir.path().join(PLAY).exists());
        assert!(dex(dir.path()).is_empty());
    }

    #[test]
    fn a_capability_keeps_its_libraries_shim_and_r_classes_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        template(dir.path());
        let entries = keep(dir.path(), &[Capability::PlayGames], "com.studio.tide")
            .unwrap()
            .unwrap();
        assert_eq!(
            dex(dir.path()),
            [
                "classes2.dex",
                "classes3.dex",
                "classes4.dex",
                "classes5.dex"
            ]
        );
        let bodies: Vec<String> = (2..=5)
            .map(|n| std::fs::read_to_string(dir.path().join(format!("classes{n}.dex"))).unwrap())
            .collect();
        assert_eq!(bodies, ["r", "games shim", "base", "games"]);
        assert_eq!(
            resources(dir.path()),
            [dir.path().join(PLAY).join(RES).join("00")]
        );
        // The ids R was generated with, under the game's own package.
        let ids = std::fs::read_to_string(dir.path().join(PLAY).join(IDS)).unwrap();
        assert!(
            ids.starts_with("com.studio.tide:integer/google_play_services_version"),
            "{ids}"
        );
        assert!(entries.application[0].contains("com.studio.tide.init"));
        assert!(!dir.path().join(PLAY).join("02-billing").exists());
    }

    #[test]
    fn two_capabilities_share_a_library_once() {
        let dir = tempfile::tempdir().unwrap();
        template(dir.path());
        let both = [Capability::InAppPurchase, Capability::PlayGames];
        let entries = keep(dir.path(), &both, "com.studio.tide").unwrap().unwrap();
        let bodies: Vec<String> = dex(dir.path())
            .iter()
            .map(|n| std::fs::read_to_string(dir.path().join(n)).unwrap())
            .collect();
        assert_eq!(
            bodies.iter().filter(|b| *b == "base").count(),
            1,
            "{bodies:?}"
        );
        assert_eq!(bodies.len(), 6, "{bodies:?}");
        assert_eq!(entries.permissions, ["com.android.vending.BILLING"]);
    }

    #[test]
    fn a_capability_on_a_template_without_libraries_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = keep(dir.path(), &[Capability::InAppReview], "com.studio.tide")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("in-app-review") && err.contains("no Google libraries"),
            "{err}"
        );
    }

    #[test]
    fn the_libraries_entries_land_beside_and_inside_the_application() {
        let dir = tempfile::tempdir().unwrap();
        template(dir.path());
        let entries = keep(dir.path(), &[Capability::PlayGames], "com.studio.tide")
            .unwrap()
            .unwrap();
        let xml = declare(super::super::test_config::TEMPLATE, &entries, Some("123")).unwrap();
        let queries = xml.find("<queries>").unwrap();
        let application = xml.find("<application").unwrap();
        let provider = xml.find("<provider").unwrap();
        let app_id = xml.find("com.google.android.gms.games.APP_ID").unwrap();
        assert!(queries < application && application < provider, "{xml}");
        assert!(app_id < xml.find("</application>").unwrap(), "{xml}");
        assert!(xml.contains("@string/game_services_project_id"), "{xml}");
    }
}
