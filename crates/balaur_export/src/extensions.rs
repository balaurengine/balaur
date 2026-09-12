//! A project's extension libraries, copied to where the exported game loads
//! them from: `standalone::extensions_beside` its executable.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use balaur::standalone::{EXTENSIONS_DIR, extensions_beside};

/// The suffix an extension library has on each desktop.
const SUFFIXES: [&str; 3] = ["so", "dylib", "dll"];

/// Every library in the project's `extensions/`, sorted.
pub(crate) fn in_project(project: &Path) -> Vec<PathBuf> {
    let dir = project.join(EXTENSIONS_DIR);
    let mut found: Vec<PathBuf> = balaur::files::default_backend()
        .list(&dir)
        .into_iter()
        .filter(|(name, is_dir)| !is_dir && suffix_of(Path::new(name)).is_some())
        .map(|(name, _)| dir.join(name))
        .collect();
    found.sort();
    found
}

fn suffix_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?;
    SUFFIXES.into_iter().find(|s| ext == *s)
}

/// Ship the project's libraries that `template` can load beside `exe`, the
/// game fused onto it.
pub(crate) fn ship_for(project: &Path, template: &[u8], exe: &Path) -> Result<Vec<PathBuf>> {
    let libraries = in_project(project);
    if let Some(suffix) = suffix_for(template) {
        return ship(&libraries, suffix, exe);
    }
    if !libraries.is_empty() {
        tracing::warn!(
            "the template is not an ELF, Mach-O or PE executable, so this game ships without {}",
            names(&libraries)
        );
    }
    Ok(Vec::new())
}

/// The suffix of the libraries `executable` can load, read off its format.
fn suffix_for(executable: &[u8]) -> Option<&'static str> {
    match executable.get(..4)? {
        [0x7f, b'E', b'L', b'F'] => Some("so"),
        // Thin Mach-O, 32 and 64 bit, then a universal binary.
        [0xce | 0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] => Some("dylib"),
        [b'M', b'Z', ..] => Some("dll"),
        _ => None,
    }
}

/// Copy the libraries ending in `suffix` to where `exe` loads them from, and
/// return where they went.
fn ship(libraries: &[PathBuf], suffix: &str, exe: &Path) -> Result<Vec<PathBuf>> {
    let wanted: Vec<&PathBuf> = libraries
        .iter()
        .filter(|l| suffix_of(l) == Some(suffix))
        .collect();
    if wanted.is_empty() {
        if !libraries.is_empty() {
            tracing::warn!(
                "{EXTENSIONS_DIR}/ has no .{suffix} library, so this game ships without {}",
                names(libraries)
            );
        }
        return Ok(Vec::new());
    }
    let dir = extensions_beside(exe);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut shipped = Vec::with_capacity(wanted.len());
    for library in wanted {
        let to = dir.join(library.file_name().context("a library with no name")?);
        // A game exported into its own project sits beside the source, and
        // copying a file onto itself truncates it.
        if to.canonicalize().ok() != library.canonicalize().ok() {
            std::fs::copy(library, &to)
                .with_context(|| format!("copying {} to {}", library.display(), to.display()))?;
        }
        shipped.push(to);
    }
    tracing::info!("shipped {} -> {}", names(&shipped), dir.display());
    Ok(shipped)
}

/// Say that a platform with no `dlopen` leaves the project's libraries behind.
pub(crate) fn warn_left_behind(libraries: &[PathBuf], platform: &str) {
    if !libraries.is_empty() {
        tracing::warn!(
            "a {platform} build cannot load extensions, so this game ships without {} and \
             a script calling one fails there; a plugin this build needs has to be a module",
            names(libraries)
        );
    }
}

fn names(libraries: &[PathBuf]) -> String {
    libraries
        .iter()
        .filter_map(|l| l.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(EXTENSIONS_DIR)).unwrap();
        for file in files {
            std::fs::write(dir.path().join(EXTENSIONS_DIR).join(file), file).unwrap();
        }
        dir
    }

    #[test]
    fn a_project_lists_only_its_libraries_in_a_stable_order() {
        let project = project_with(&["b.so", "a.dylib", "notes.txt", "c.dll"]);
        let found: Vec<String> = in_project(project.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["a.dylib", "b.so", "c.dll"]);
        assert!(in_project(&project.path().join("absent")).is_empty());
    }

    #[test]
    fn a_template_says_which_libraries_it_loads() {
        assert_eq!(suffix_for(b"\x7fELF\x02\x01"), Some("so"));
        assert_eq!(suffix_for(&[0xcf, 0xfa, 0xed, 0xfe, 7]), Some("dylib"));
        assert_eq!(suffix_for(&[0xca, 0xfe, 0xba, 0xbe, 0]), Some("dylib"));
        assert_eq!(suffix_for(b"MZ\x90\x00"), Some("dll"));
        assert_eq!(suffix_for(b"#!/bin/sh"), None);
        assert_eq!(suffix_for(b"MZ"), None, "too short to be an executable");
    }

    #[test]
    fn a_flat_game_gets_its_platforms_libraries_beside_it() {
        let project = project_with(&["greeter.so", "greeter.dylib"]);
        let out = tempfile::tempdir().unwrap();
        let exe = out.path().join("game");

        let shipped = ship(&in_project(project.path()), "so", &exe).unwrap();

        assert_eq!(shipped, [out.path().join("extensions/greeter.so")]);
        assert_eq!(std::fs::read(&shipped[0]).unwrap(), b"greeter.so");
        assert!(!out.path().join("extensions/greeter.dylib").exists());
    }

    #[test]
    fn a_macos_app_gets_its_libraries_in_plugins() {
        let project = project_with(&["greeter.dylib"]);
        let out = tempfile::tempdir().unwrap();
        let exe = out.path().join("Game.app/Contents/MacOS/Game");

        let shipped = ship(&in_project(project.path()), "dylib", &exe).unwrap();

        assert_eq!(
            shipped,
            [out.path().join("Game.app/Contents/PlugIns/greeter.dylib")]
        );
        assert!(shipped[0].is_file());
    }

    #[test]
    fn exporting_into_the_project_leaves_its_libraries_intact() {
        let project = project_with(&["greeter.so"]);
        let exe = project.path().join("game");

        ship(&in_project(project.path()), "so", &exe).unwrap();

        assert_eq!(
            std::fs::read(project.path().join("extensions/greeter.so")).unwrap(),
            b"greeter.so"
        );
    }

    #[test]
    fn a_target_with_no_library_of_its_own_ships_nothing() {
        let project = project_with(&["greeter.dylib"]);
        let out = tempfile::tempdir().unwrap();

        let shipped = ship(
            &in_project(project.path()),
            "dll",
            &out.path().join("game.exe"),
        );

        assert!(shipped.unwrap().is_empty());
        assert!(!out.path().join("extensions").exists());
    }
}
