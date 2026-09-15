//! `balaur update`: replace this install with the newest build on its
//! channel, which is the prerelease identifier of its own version
//! (`docs/RELEASING.md`).
//!
//! The binary, the bundled `editor/` project, the runtime template and the C
//! header ship as one archive and only work as a set, so updating is
//! replacing the whole install directory's contents, never one file.

#[cfg(target_family = "wasm")]
pub(crate) fn run(_opts: &crate::UpdateOpts) -> anyhow::Result<()> {
    anyhow::bail!("updating is not available in this build")
}

#[cfg(not(target_family = "wasm"))]
pub(crate) use imp::{install, releases, run};

/// One published release, as the feed lists it.
pub(crate) struct Release {
    /// The tag the release was cut under, `v0.2.0` or `nightly`.
    pub(crate) tag: String,
    /// The build id its assets carry, which for a rolling tag is not the tag
    /// itself.
    pub(crate) id: String,
    /// When it was published, as the feed's own ISO 8601 string.
    pub(crate) published: String,
    /// Which line it belongs to, from the id's own prerelease name.
    pub(crate) channel: String,
}

#[cfg(target_family = "wasm")]
pub(crate) fn releases() -> anyhow::Result<Vec<Release>> {
    anyhow::bail!("a tab runs the build the page served it")
}

#[cfg(target_family = "wasm")]
pub(crate) fn install(
    _tag: Option<&str>,
    _channel: Option<&str>,
    _allow_downgrade: bool,
) -> anyhow::Result<String> {
    anyhow::bail!("updating is not available in this build")
}

#[cfg(not(target_family = "wasm"))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::Release;

    use anyhow::{Context, Result, bail};

    const RELEASE_BASE: &str = "https://github.com/balaurengine/balaur/releases";

    /// The published editor build for the machine this runs on.
    fn host_target() -> Result<&'static str> {
        if cfg!(target_os = "macos") {
            Ok("macos-universal")
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok("windows-x64")
        } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
            Ok("windows-arm64")
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok("linux-x64")
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            Ok("linux-arm64")
        } else {
            bail!("no published build for this platform; update from source instead")
        }
    }

    /// Where a channel's releases are: a rolling tag per prerelease line,
    /// and GitHub's `latest` for stable, which skips the prereleases.
    fn channel_base(channel: &str) -> String {
        if channel == crate::version::STABLE {
            format!("{RELEASE_BASE}/latest/download")
        } else {
            format!("{RELEASE_BASE}/download/{channel}")
        }
    }

    /// Where an update reads from: one line, followed; or one release, named.
    enum Source {
        Channel(String),
        Tag(String),
    }

    impl Source {
        fn base(&self) -> String {
            match self {
                Self::Channel(channel) => channel_base(channel),
                Self::Tag(tag) => format!("{RELEASE_BASE}/download/{tag}"),
            }
        }

        /// Why there was no VERSION to read. A channel names the ones that do
        /// have a release, since the fix is almost always another channel.
        fn missing(&self) -> String {
            let Self::Channel(channel) = self else {
                return format!("{self} publishes no VERSION asset");
            };
            let live: Vec<&str> = crate::version::CHANNELS
                .iter()
                .copied()
                .filter(|other| *other != channel && has_a_release(other))
                .collect();
            if live.is_empty() {
                format!("nothing is published on {self}")
            } else {
                format!(
                    "nothing is published on {self}; try --channel {}",
                    live.join(" or --channel ")
                )
            }
        }
    }

    impl Source {
        /// The version an update would install, said with where it came from
        /// when that is not already the name the caller used.
        fn installing(&self, published: &str) -> String {
            match self {
                Self::Channel(_) => format!("{published} on {self}"),
                Self::Tag(_) => published.to_string(),
            }
        }
    }

    impl std::fmt::Display for Source {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Channel(channel) => write!(f, "the {channel} channel"),
                Self::Tag(tag) => write!(f, "release {tag}"),
            }
        }
    }

    /// Whether a directory is cargo's own output rather than an install.
    fn build_tree(dir: &Path) -> bool {
        dir.join(".fingerprint").is_dir() || dir.join("incremental").is_dir()
    }

    /// Only for a failure message, so the cost lands on a path that has
    /// already failed.
    fn has_a_release(channel: &str) -> bool {
        let url = format!("{}/VERSION", channel_base(channel));
        matches!(crate::templates::fetch_text(&url), Ok(Some(_)))
    }

    /// `--tag` names one release, `--channel` one line, and a build with
    /// neither follows the line its own version names.
    fn source(tag: Option<&str>, channel: Option<&str>) -> Result<Source> {
        if let Some(tag) = tag {
            return Ok(Source::Tag(tag.to_string()));
        }
        if let Some(channel) = channel {
            if !crate::version::CHANNELS.contains(&channel) {
                bail!(
                    "no channel named {channel}; the channels are {}",
                    crate::version::CHANNELS.join(", ")
                );
            }
            return Ok(Source::Channel(channel.to_string()));
        }
        let own = crate::version::channel().context(
            "this is a source build; update it with git and cargo, or pass --channel or --tag",
        )?;
        Ok(Source::Channel(own.to_string()))
    }

    /// The release the archives come from. A channel's rolling tag carries
    /// only VERSION, naming the release that holds the rest; a nightly names
    /// a build rather than a tag, so its assets stay where VERSION was.
    fn assets_base(base: &str, published: &str) -> String {
        if crate::version::channel_of(published) == crate::version::NIGHTLY {
            base.to_string()
        } else {
            format!("{RELEASE_BASE}/download/{published}")
        }
    }

    /// Every release the project has published, newest first. The feed is
    /// public, so this asks for no token and sends none.
    pub(crate) fn releases() -> Result<Vec<Release>> {
        const FEED: &str = "https://api.github.com/repos/balaurengine/balaur/releases?per_page=30";
        let text =
            crate::templates::fetch_text(FEED)?.context("the release feed answered nothing")?;
        let feed: serde_json::Value = serde_json::from_str(&text)?;
        let items = feed.as_array().context("the release feed is not a list")?;
        let mut out = Vec::new();
        for item in items {
            if item.get("draft").and_then(serde_json::Value::as_bool) == Some(true) {
                continue;
            }
            let Some(tag) = item.get("tag_name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            // A rolling tag names a build in its VERSION asset rather than in
            // the tag, so the id is read from the release's own name when it
            // has one and falls back to the tag.
            let id = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| name.starts_with('v') || name.starts_with("nightly"))
                .unwrap_or(tag);
            out.push(Release {
                channel: crate::version::channel_of(id).to_string(),
                published: item
                    .get("published_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                id: id.to_string(),
                tag: tag.to_string(),
            });
        }
        // A rolling tag says nothing about which build it holds; its VERSION
        // asset does. One read per rolling line, not per release.
        for release in &mut out {
            if release.id != release.tag {
                continue;
            }
            if let Ok(Some(found)) = looked_up(Some(&release.tag), None) {
                release.id = found.id;
            }
        }
        Ok(out)
    }

    /// What a source is publishing now, read once over the network.
    pub(crate) struct Published {
        /// The build id the source holds, `v0.2.0` or `nightly-<sha>`.
        pub(crate) id: String,
        /// How to say where it came from, for a line a person reads.
        pub(crate) source: String,
        /// Whether installing it would be a step back from this build.
        pub(crate) downgrade: bool,
    }

    /// Ask a channel or a tag what it holds. One network read, no install.
    /// `None` is a source that publishes nothing yet, which is an answer
    /// rather than a failure: a line can exist before it has a release.
    pub(crate) fn looked_up(tag: Option<&str>, channel: Option<&str>) -> Result<Option<Published>> {
        let source = source(tag, channel)?;
        let Some(text) = crate::templates::fetch_text(&format!("{}/VERSION", source.base()))?
        else {
            return Ok(None);
        };
        let id = text.trim().to_string();
        let own = crate::version::build_id().unwrap_or("dev");
        Ok(Some(Published {
            downgrade: crate::version::is_downgrade(&id, own),
            source: source.to_string(),
            id,
        }))
    }

    /// The same read, where nothing published is an error: what a command
    /// line wants, with the other channels named in the message.
    fn published(tag: Option<&str>, channel: Option<&str>) -> Result<Published> {
        let source = source(tag, channel)?;
        looked_up(tag, channel)?.with_context(|| source.missing())
    }

    /// Replace this install with what a channel or a tag holds. Answers the
    /// line to show when it worked.
    pub(crate) fn install(
        tag: Option<&str>,
        channel: Option<&str>,
        allow_downgrade: bool,
    ) -> Result<String> {
        let source = source(tag, channel)?;
        let base = source.base();
        let own = crate::version::build_id().unwrap_or("dev");
        let found = published(tag, channel)?;
        let published = found.id.as_str();
        if published == own {
            return Ok(format!("already up to date on {source} ({own})"));
        }
        if !allow_downgrade && found.downgrade {
            bail!(
                "{} is older than the installed {own}; \
                 allow a downgrade to install it anyway",
                source.installing(published)
            );
        }
        let exe = std::env::current_exe().context("locating the running executable")?;
        let install = exe
            .parent()
            .context("the executable has no parent directory")?
            .to_path_buf();
        // The archive this unpacks has the layout of a plain download, not a
        // bundle's, and the ticket stapled to the .dmg covers what it replaces.
        if install.ends_with("Contents/MacOS") {
            bail!("Balaur.app updates by downloading the new .dmg, not in place");
        }
        // A cargo target directory is a build tree rather than an install:
        // a release unpacked over it buries what the build wrote, and the
        // editor's Engine tab is one press away from asking for that.
        if build_tree(&install) {
            bail!(
                "{} is a build tree, not an install; update a source build with git",
                install.display()
            );
        }
        let staged = download_and_unpack(&assets_base(&base, published), &install)?;
        swap_install(&staged, &install, &exe)?;
        std::fs::remove_dir_all(&staged).ok();
        Ok(format!("updated to {published}; restart to use it"))
    }

    pub(crate) fn run(opts: &crate::UpdateOpts) -> Result<()> {
        let found = published(opts.tag.as_deref(), opts.channel.as_deref())?;
        let own = crate::version::build_id().unwrap_or("dev");
        tracing::info!("installed: {own}; {}: {}", found.source, found.id);
        if opts.check {
            return Ok(());
        }
        let note = install(
            opts.tag.as_deref(),
            opts.channel.as_deref(),
            opts.allow_downgrade,
        )?;
        tracing::info!("{note}");
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
        /// Reaching a source build's own channel needs a baked-in id, which
        /// a cargo build has none of.
        fn base(tag: Option<&str>, channel: Option<&str>) -> String {
            super::source(tag, channel)
                .expect("a tag or a channel needs no build id")
                .base()
        }

        #[test]
        fn a_tag_names_one_release_and_a_channel_names_a_line() {
            assert!(base(Some("v9.9.9"), None).ends_with("/download/v9.9.9"));
            assert!(base(None, Some("alpha")).ends_with("/download/alpha"));
            assert!(base(None, Some("stable")).ends_with("/latest/download"));
            assert!(
                super::source(None, Some("edge")).is_err(),
                "a channel that does not exist must refuse"
            );
            assert!(
                super::source(None, None).is_err(),
                "a source build must refuse"
            );
        }

        #[test]
        fn a_channel_sends_the_download_to_the_release_its_version_names() {
            let base = base(None, Some("alpha"));
            let named = super::assets_base(&base, "v0.3.0-alpha.1");
            assert!(named.ends_with("/download/v0.3.0-alpha.1"));
            assert_eq!(super::assets_base(&base, "nightly-abc1234"), base);
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
