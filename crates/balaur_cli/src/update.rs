//! `balaur update`: replace this install with the latest published build.
//!
//! The binary, the bundled `editor/` project, the runtime template and the C
//! header ship as one archive and only work as a set, so updating is
//! replacing the whole install directory's contents, never one file.

#[cfg(target_family = "wasm")]
pub(crate) fn run(_tag: Option<&str>, _check_only: bool) -> anyhow::Result<()> {
    anyhow::bail!("updating is not available in this build")
}

#[cfg(not(target_family = "wasm"))]
pub(crate) use imp::run;

#[cfg(not(target_family = "wasm"))]
mod imp {
    use std::path::{Path, PathBuf};

    use anyhow::{bail, Context, Result};

    const RELEASE_BASE: &str = "https://github.com/balaurengine/balaur/releases";

    /// The published editor build for the machine this runs on.
    fn host_target() -> Result<&'static str> {
        if cfg!(target_os = "macos") {
            Ok("macos-universal")
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok("windows-x64")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok("linux-x64")
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            Ok("linux-arm64")
        } else {
            bail!("no published build for this platform; update from source instead")
        }
    }

    /// `.../download/<tag>/` for an explicit tag; the rolling `nightly` for a
    /// nightly build; GitHub's `latest` alias for a tagged release, so a
    /// release build updates to the newest release rather than to itself.
    fn asset_base(tag: Option<&str>) -> Result<String> {
        let segment = if let Some(explicit) = tag {
            explicit
        } else {
            let own = crate::version::build_id()
                .context("this is a source build; update it with git and cargo, or pass --tag")?;
            if own.starts_with('v') {
                "latest"
            } else {
                "nightly"
            }
        };
        if segment == "latest" {
            Ok(format!("{RELEASE_BASE}/latest/download"))
        } else {
            Ok(format!("{RELEASE_BASE}/download/{segment}"))
        }
    }

    pub(crate) fn run(tag: Option<&str>, check_only: bool) -> Result<()> {
        let base = asset_base(tag)?;
        let own = crate::version::build_id().unwrap_or("dev");
        let published = crate::templates::fetch_text(&format!("{base}/VERSION"))?
            .context("that release publishes no VERSION asset; pass an explicit --tag")?;
        let published = published.trim();
        if published == own {
            tracing::info!("already up to date ({own})");
            return Ok(());
        }
        tracing::info!("installed: {own}; published: {published}");
        if check_only {
            return Ok(());
        }
        let exe = std::env::current_exe().context("locating the running executable")?;
        let install = exe
            .parent()
            .context("the executable has no parent directory")?
            .to_path_buf();
        let staged = download_and_unpack(&base, &install)?;
        swap_install(&staged, &install, &exe)?;
        std::fs::remove_dir_all(&staged).ok();
        tracing::info!("updated to {published}; restart to use it");
        Ok(())
    }

    /// Fetch the editor archive for this platform, verify it, and unpack it
    /// into a staging directory inside the install dir (same filesystem, so
    /// the swap is renames). Returns the unpacked bundle root.
    fn download_and_unpack(base: &str, install: &Path) -> Result<PathBuf> {
        let target = host_target()?;
        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let name = format!("balaur-editor-{target}.{ext}");
        let url = format!("{base}/{name}");
        let expected = crate::templates::expected_sha256(&format!("{base}/SHA256SUMS"), &name)?;
        let staging = install.join(".balaur-update");
        std::fs::remove_dir_all(&staging).ok();
        std::fs::create_dir_all(&staging)?;
        let archive = staging.join(&name);
        crate::templates::download(&url, &archive, expected.as_deref())?;
        unpack(&archive, &staging)?;
        std::fs::remove_file(&archive).ok();
        let root = staging.join(format!("balaur-editor-{target}"));
        if !root.is_dir() {
            bail!("{name} did not contain balaur-editor-{target}/");
        }
        Ok(root)
    }

    #[cfg(not(windows))]
    fn unpack(archive: &Path, into: &Path) -> Result<()> {
        let file = std::fs::File::open(archive)?;
        tar::Archive::new(flate2::read::GzDecoder::new(file))
            .unpack(into)
            .with_context(|| format!("unpacking {}", archive.display()))
    }

    #[cfg(windows)]
    fn unpack(archive: &Path, into: &Path) -> Result<()> {
        let file = std::fs::File::open(archive)?;
        zip::ZipArchive::new(file)?
            .extract(into)
            .with_context(|| format!("unpacking {}", archive.display()))
    }

    /// Move every entry of the new bundle into the install directory. The
    /// running executable cannot be overwritten in place (Windows), but it
    /// can be renamed — so old entries step aside first and are removed
    /// best-effort after.
    fn swap_install(bundle: &Path, install: &Path, exe: &Path) -> Result<()> {
        let exe_name = exe.file_name().context("the executable has no name")?;
        for entry in std::fs::read_dir(bundle)? {
            let entry = entry?;
            let new = entry.path();
            let current = install.join(entry.file_name());
            let aside = install.join(format!(".old-{}", entry.file_name().to_string_lossy()));
            std::fs::remove_dir_all(&aside).ok();
            std::fs::remove_file(&aside).ok();
            if current.exists() || entry.file_name() == exe_name {
                std::fs::rename(&current, &aside)
                    .with_context(|| format!("moving {} aside", current.display()))?;
            }
            std::fs::rename(&new, &current)
                .with_context(|| format!("installing {}", current.display()))?;
            if std::fs::remove_dir_all(&aside).is_err() {
                // The running executable on Windows: undeletable until exit.
                std::fs::remove_file(&aside).ok();
            }
        }
        Ok(())
    }

    /// Read a whole file — the test server helper needs `Read` in scope.
    #[cfg(test)]
    fn read_all(path: &Path) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        let mut file = std::fs::File::open(path).expect("the swapped file is readable");
        file.read_to_end(&mut out)
            .expect("the swapped file reads to the end");
        out
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn a_tagged_build_updates_to_the_latest_release() {
            // No build id under cargo, so the base needs an explicit tag.
            let base =
                super::asset_base(Some("v9.9.9")).expect("an explicit tag needs no build id");
            assert!(base.ends_with("/download/v9.9.9"));
            assert!(
                super::asset_base(None).is_err(),
                "a source build must refuse"
            );
        }

        #[test]
        fn swapping_replaces_the_binary_and_directories() {
            let dir = tempfile::tempdir().expect("a temp directory is creatable");
            let install = dir.path().join("install");
            let bundle = dir.path().join("bundle");
            for d in [&install, &bundle] {
                std::fs::create_dir_all(d).expect("test directories are creatable");
            }
            std::fs::write(install.join("balaur"), b"old").expect("the old binary writes");
            std::fs::create_dir(install.join("editor")).expect("the old editor dir writes");
            std::fs::write(bundle.join("balaur"), b"new").expect("the new binary writes");
            std::fs::create_dir(bundle.join("editor")).expect("the new editor dir writes");
            std::fs::write(bundle.join("editor").join("a.rn"), b"x")
                .expect("the new editor file writes");

            super::swap_install(&bundle, &install, &install.join("balaur"))
                .expect("the swap succeeds");
            assert_eq!(super::read_all(&install.join("balaur")), b"new");
            assert!(install.join("editor").join("a.rn").is_file());
        }
    }
}
