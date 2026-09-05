//! The per-user template cache, and downloading a missing template into it.

use std::path::PathBuf;

/// Downloaded templates live per user, never inside a project:
/// `<platform data dir>/balaur/templates/<build id>`. Keyed by the exact
/// build because a template must match the engine that compiled the pack.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn cache_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| {
        d.join("balaur")
            .join("templates")
            .join(crate::version::build_id().unwrap_or("dev"))
    })
}

#[cfg(target_family = "wasm")]
pub(crate) fn cache_dir() -> Option<PathBuf> {
    None
}

#[cfg(target_family = "wasm")]
pub(crate) fn obtain(_target: &str, _assume_yes: bool) -> anyhow::Result<PathBuf> {
    anyhow::bail!("template download is not available in this build")
}

#[cfg(not(target_family = "wasm"))]
pub(crate) use fetch::{download, expected_sha256, fetch_text, obtain};

#[cfg(not(target_family = "wasm"))]
mod fetch {
    // Anonymous: `std::io::Write` is here too, and only the `write!` into a
    // String needs the `fmt` one.
    use std::fmt::Write as _;
    use std::io::{IsTerminal, Read, Write};
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result, bail};
    use sha2::Digest;

    const RELEASE_BASE: &str = "https://github.com/balaurengine/balaur/releases/download";

    /// The tag this build's own assets live under, so a pack only ever meets
    /// the runtime its compiler shipped with. BALAUR_TEMPLATE_TAG overrides.
    fn release_tag() -> Result<String> {
        if let Ok(tag) = std::env::var("BALAUR_TEMPLATE_TAG") {
            return Ok(tag);
        }
        crate::version::release_tag().map(str::to_string).context(
            "this is a source build with no release to download from; build the \
             template yourself (cargo build --release -p balaur_cli), pass \
             --template <file>, or set BALAUR_TEMPLATE_TAG",
        )
    }

    fn asset_name(target: &str) -> String {
        if target.starts_with("windows-") {
            format!("balaur-runtime-{target}.exe")
        } else {
            format!("balaur-runtime-{target}")
        }
    }

    /// Download the template for `target` into the cache and return its path.
    /// Asks first on a terminal; without one it refuses unless `assume_yes`
    /// (`--download`), so CI never fetches by surprise.
    pub(crate) fn obtain(target: &str, assume_yes: bool) -> Result<PathBuf> {
        let name = asset_name(target);
        let tag = release_tag()?;
        let url = format!("{RELEASE_BASE}/{tag}/{name}");
        let dir = super::cache_dir().context("no user data directory on this platform")?;
        confirm(&name, &url, &dir, assume_yes)?;
        refuse_a_stale_nightly(&tag)?;
        std::fs::create_dir_all(&dir)?;
        let expected = expected_sha256(&format!("{RELEASE_BASE}/{tag}/SHA256SUMS"), &name)?;
        if expected.is_none() {
            tracing::warn!("release {tag} publishes no SHA256SUMS; skipping verification");
        }
        let path = dir.join(&name);
        download(&url, &path, expected.as_deref())?;
        tracing::info!("downloaded {name} -> {}", path.display());
        Ok(path)
    }

    fn confirm(name: &str, url: &str, dir: &Path, assume_yes: bool) -> Result<()> {
        if assume_yes {
            return Ok(());
        }
        if !std::io::stdin().is_terminal() {
            bail!(
                "template {name} is not installed; pass --download to fetch it \
                 from {url} into {}",
                dir.display()
            );
        }
        eprint!(
            "Template {name} is not installed. Download it from\n{url}\ninto {}? [y/N] ",
            dir.display()
        );
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            bail!("not downloading; install the template manually or pass --template <file>");
        }
        Ok(())
    }

    /// The rolling `nightly` tag moves: a template published after this
    /// binary was built comes from a different compiler, and fusing the two
    /// is undefined. The release's VERSION asset names the build it holds.
    fn refuse_a_stale_nightly(tag: &str) -> Result<()> {
        let Some(own) = crate::version::build_id().filter(|id| !id.starts_with('v')) else {
            return Ok(());
        };
        match fetch_text(&format!("{RELEASE_BASE}/{tag}/VERSION"))? {
            Some(published) if published.trim() != own => bail!(
                "this build is {own} but the published {tag} release is {}; \
                 run `balaur update` first",
                published.trim()
            ),
            _ => Ok(()),
        }
    }

    /// A small text asset from the release, or None when the release does
    /// not carry it.
    pub(crate) fn fetch_text(url: &str) -> Result<Option<String>> {
        let mut response = agent().get(url).call()?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            bail!("fetching {url}: HTTP {}", response.status());
        }
        Ok(Some(response.body_mut().read_to_string()?))
    }

    fn agent() -> ureq::Agent {
        // A 404 needs its own message (no release for this tag), so statuses
        // are handled here rather than surfacing as transport errors.
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into()
    }

    /// The hash for `name` out of the release's SHA256SUMS, or None when the
    /// release predates checksums.
    pub(crate) fn expected_sha256(sums_url: &str, name: &str) -> Result<Option<String>> {
        let Some(sums) = fetch_text(sums_url)? else {
            return Ok(None);
        };
        Ok(sums.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            (parts.next()? == name).then(|| hash.to_ascii_lowercase())
        }))
    }

    /// Stream `url` to `path`, verified against `expected` when given. The
    /// bytes land in a sibling `.partial` first, so an interrupted or
    /// rejected download never looks installed.
    pub(crate) fn download(url: &str, path: &Path, expected: Option<&str>) -> Result<()> {
        let mut response = agent().get(url).call()?;
        if response.status().as_u16() == 404 {
            bail!("{url} does not exist — is this engine version published as a release?");
        }
        if !response.status().is_success() {
            bail!("fetching {url}: HTTP {}", response.status());
        }
        let partial = path.with_extension("partial");
        let mut file = std::fs::File::create(&partial)
            .with_context(|| format!("creating {}", partial.display()))?;
        let mut reader = response.body_mut().as_reader();
        let mut hasher = sha2::Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
        }
        drop(file);
        // sha2 0.11 hands back an `Array`, which has no `LowerHex`; folded
        // rather than mapped into Strings, as `pack::content_hash` does.
        let got = hasher.finalize().iter().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        });
        if let Some(expected) = expected {
            if got != expected {
                std::fs::remove_file(&partial).ok();
                bail!("checksum mismatch for {url}: expected {expected}, got {got}");
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&partial, path)?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use std::io::{Read, Write};

        fn serve_once(body: &'static [u8]) -> String {
            let listener =
                std::net::TcpListener::bind("127.0.0.1:0").expect("an ephemeral local port binds");
            let addr = listener
                .local_addr()
                .expect("a bound listener has an address");
            std::thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let head = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                }
            });
            format!("http://{addr}/asset")
        }

        const HELLO_SHA256: &str =
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

        #[test]
        fn a_windows_target_downloads_the_exe_asset() {
            assert_eq!(
                super::asset_name("windows-x64"),
                "balaur-runtime-windows-x64.exe"
            );
            assert_eq!(super::asset_name("linux-x64"), "balaur-runtime-linux-x64");
        }

        #[test]
        fn the_cache_is_keyed_by_the_build_id() {
            // Under cargo no build id is baked in, so the cache says `dev`.
            let dir = crate::templates::cache_dir().expect("desktop platforms have a data dir");
            assert!(dir.ends_with("balaur/templates/dev"));
        }

        #[test]
        fn a_verified_download_lands_at_the_final_path() {
            let dir = tempfile::tempdir().expect("a temp directory is creatable");
            let path = dir.path().join("balaur-runtime-test");
            super::download(&serve_once(b"hello"), &path, Some(HELLO_SHA256))
                .expect("a matching checksum accepts the download");
            let body = std::fs::read(&path).expect("the downloaded file is readable");
            assert_eq!(body, b"hello");
        }

        #[test]
        fn a_checksum_mismatch_rejects_the_download_and_leaves_no_file() {
            let dir = tempfile::tempdir().expect("a temp directory is creatable");
            let path = dir.path().join("balaur-runtime-test");
            let Err(err) = super::download(&serve_once(b"tampered"), &path, Some(HELLO_SHA256))
            else {
                panic!("a wrong checksum must be rejected")
            };
            assert!(err.to_string().contains("checksum mismatch"), "{err}");
            assert!(!path.exists());
            assert!(!path.with_extension("partial").exists());
        }
    }
}
