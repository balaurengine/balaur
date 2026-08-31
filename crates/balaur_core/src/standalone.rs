//! Standalone games: a pack carried inside the executable that runs it.
//!
//! A `.bpak` is data: it needs an engine binary to run it. Handing a player two
//! files and a command line is not shipping a game, so `balaur export
//! --target <platform>` takes a *template* — the engine binary CI built for
//! that platform — and appends the pack to it:
//!
//! ```text
//! [ template executable ][ pack bytes ][ pack length: u64 LE ][ "BPAKSELF" ]
//! ```
//!
//! Every desktop executable format (ELF, Mach-O, PE) ignores trailing bytes,
//! so the result still runs. On startup the engine reads its own file, finds
//! the trailer, and boots the pack instead of parsing a command line — the
//! same binary is the CLI when nothing is appended and the game when something
//! is.
//!
//! A signature can never cover appended bytes (codesign rewrites the file and
//! fails strict validation), so a *signed* macOS game is a `.app` bundle with
//! the pack in Contents/Resources — `balaur export --app`.

use std::path::Path;

use anyhow::{Context, Result};

/// Trailer marker. Deliberately not the `.bpak` magic: this identifies an
/// executable carrying a pack, and mistaking one for the other should be
/// impossible.
const MAGIC: &[u8; 8] = b"BPAKSELF";
/// Pack length plus the marker.
const TRAILER: usize = 8 + MAGIC.len();

/// Append `pack` to `template`, producing a standalone game executable.
pub fn build(template: &[u8], pack: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(template.len() + pack.len() + TRAILER);
    out.extend_from_slice(template);
    out.extend_from_slice(pack);
    out.extend_from_slice(&(pack.len() as u64).to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

/// The pack carried in `bytes`, or `None` if this is a plain executable.
///
/// Every failure is `None` rather than an error: an ordinary binary ends in
/// whatever it ends in, and that is not a corrupt game, just not a game.
#[must_use]
pub fn extract(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < TRAILER {
        return None;
    }
    let (head, marker) = bytes.split_at(bytes.len() - MAGIC.len());
    if marker != MAGIC {
        return None;
    }
    let (head, len) = head.split_at(head.len() - 8);
    let len = u64::from_le_bytes(len.try_into().ok()?);
    let len = usize::try_from(len).ok()?;
    // A length longer than the file is a truncated or forged trailer, not a
    // pack; refusing beats panicking on the slice.
    head.len().checked_sub(len).map(|start| &head[start..])
}

/// Where a bundled game keeps its pack, next to the executable.
pub const BUNDLED_PACK: &str = "game.bpak";

/// The pack carried by the running executable, if this is a shipped game.
///
/// Desktop games carry it appended. A signed bundle cannot: appending is
/// exactly what invalidates a signature — so on iOS, and on macOS when the
/// executable lives inside a `.app`, the pack is a resource beside the
/// executable. A flat desktop binary never looks beside itself, because a
/// stray `game.bpak` next to the CLI must not silently turn it into a game.
pub fn own_pack() -> Result<Option<Vec<u8>>> {
    let exe = std::env::current_exe().context("locating the running executable")?;
    if let Some(pack) = extract_from(&exe)? {
        return Ok(Some(pack));
    }
    // An iOS bundle is flat: the pack sits beside the executable. A macOS
    // .app keeps it in Contents/Resources, where codesign seals data files.
    #[cfg(target_os = "ios")]
    let bundled = exe.parent().map(|dir| dir.join(BUNDLED_PACK));
    #[cfg(target_os = "macos")]
    let bundled = macos_bundled_pack(&exe);
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    if let Some(pack) = bundled.filter(|p| p.is_file()) {
        return Ok(Some(std::fs::read(&pack).with_context(|| {
            format!("reading the bundled pack {}", pack.display())
        })?));
    }
    Ok(None)
}

/// `Contents/Resources/game.bpak`, but only for an executable that really is
/// inside `<Name>.app/Contents/MacOS/`.
#[cfg(target_os = "macos")]
fn macos_bundled_pack(exe: &Path) -> Option<std::path::PathBuf> {
    let macos_dir = exe
        .parent()
        .filter(|d| d.file_name() == Some("MacOS".as_ref()))?;
    let contents = macos_dir
        .parent()
        .filter(|d| d.file_name() == Some("Contents".as_ref()))?;
    contents
        .parent()
        .filter(|d| d.extension() == Some("app".as_ref()))?;
    Some(contents.join("Resources").join(BUNDLED_PACK))
}

/// `own_pack`, against a named file — the same logic a test can drive.
pub fn extract_from(exe: &Path) -> Result<Option<Vec<u8>>> {
    // Read once and slice: a game pack is a handful of megabytes, and the
    // alternative is three seeks into a file that may be being replaced.
    // A binary we cannot read is not a standalone game; let the CLI carry on.
    let Ok(bytes) = std::fs::read(exe) else {
        return Ok(None);
    };
    Ok(extract(&bytes).map(<[u8]>::to_vec))
}

/// Write a standalone executable, and make sure it can actually be run.
///
/// The template's own mode is a starting point, not the answer: a template
/// that arrived through a zip or a CI artifact store has usually lost its
/// executable bit, and inheriting that produces a game the player cannot
/// launch. The execute bits go on regardless.
pub fn write_executable(path: &Path, bytes: &[u8], template: &Path) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(template).map_or(0o644, |m| m.permissions().mode());
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755))
            .with_context(|| format!("making {} executable", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = template;
    Ok(())
}
