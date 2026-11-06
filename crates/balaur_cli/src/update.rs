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
pub(crate) use imp::run;
#[cfg(all(not(target_family = "wasm"), feature = "editor"))]
pub(crate) use imp::{download_url, held, releases, replace};

/// One published release, as the feed lists it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Release {
    /// The tag the release was cut under, `v0.2.0` or `nightly`.
    pub(crate) tag: String,
    /// The build id its assets carry, which for the nightly is not the tag
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

/// How far an install has got, for a screen that shows it.
pub(crate) enum Step {
    /// Bytes of the archive so far, and its size, zero when the server did
    /// not say.
    Downloading(u64, u64),
    Unpacking,
}

#[cfg(target_family = "wasm")]
pub(crate) fn replace(
    _tag: Option<&str>,
    _channel: Option<&str>,
    _allow_downgrade: bool,
    _progress: &mut dyn FnMut(Step),
) -> anyhow::Result<String> {
    anyhow::bail!("updating is not available in this build")
}

#[cfg(target_family = "wasm")]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the native `held` answers None when an install may replace itself"
)]
pub(crate) fn held() -> Option<String> {
    Some("a browser tab runs the build the page served it".to_string())
}

#[cfg(target_family = "wasm")]
pub(crate) fn download_url(_tag: &str) -> String {
    String::new()
}

#[cfg(not(target_family = "wasm"))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{Release, Step};

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

    /// Where stable and the nightly are: GitHub's `latest`, which skips
    /// prereleases, and the `nightly` tag.
    fn channel_base(channel: &str) -> String {
        if channel == crate::version::STABLE {
            format!("{RELEASE_BASE}/latest/download")
        } else {
            format!("{RELEASE_BASE}/download/{channel}")
        }
    }

    /// The newest release of a prerelease line (`alpha`, `beta`, `rc`): its
    /// tag names the line, and the feed lists the newest first.
    fn newest_on(feed: &[Release], line: &str) -> Option<String> {
        feed.iter()
            .find(|release| release.channel == line)
            .map(|release| release.tag.clone())
    }

    /// The newest release of a prerelease line, read from the feed.
    fn newest_on_line(line: &str) -> Result<Option<String>> {
        Ok(newest_on(&feed()?, line))
    }

    /// Where an update reads from: stable or the nightly, followed; a
    /// prerelease line, at its newest release; or one release, named.
    enum Source {
        Channel(String),
        Line(String, String),
        Tag(String),
    }

    impl Source {
        fn base(&self) -> String {
            match self {
                Self::Channel(channel) => channel_base(channel),
                Self::Line(_, tag) | Self::Tag(tag) => format!("{RELEASE_BASE}/download/{tag}"),
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
                Self::Channel(_) | Self::Line(..) => format!("{published} on {self}"),
                Self::Tag(_) => published.to_string(),
            }
        }
    }

    impl std::fmt::Display for Source {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Channel(channel) | Self::Line(channel, _) => {
                    write!(f, "the {channel} channel")
                }
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
        if crate::version::is_line(channel) {
            return matches!(newest_on_line(channel), Ok(Some(_)));
        }
        let url = format!("{}/VERSION", channel_base(channel));
        matches!(crate::runtimes::fetch_text(&url), Ok(Some(_)))
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
            return line_or_channel(channel);
        }
        let own = crate::version::channel().context(
            "this is a source build; update it with git and cargo, or pass --channel or --tag",
        )?;
        line_or_channel(own)
    }

    /// A prerelease line at its newest release, or stable or the nightly. A
    /// line with nothing published stays a channel, so the read finds no
    /// VERSION and says which channels do have one.
    fn line_or_channel(channel: &str) -> Result<Source> {
        if crate::version::is_line(channel)
            && let Some(tag) = newest_on_line(channel)?
        {
            return Ok(Source::Line(channel.to_string(), tag));
        }
        Ok(Source::Channel(channel.to_string()))
    }

    /// The release the archives come from: the version VERSION names, or for
    /// a nightly, which names a build rather than a tag, where VERSION was.
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
        let mut out = feed()?;
        // A version tag is its own build id; only the nightly has to be
        // asked, through its VERSION, which build it holds now.
        for release in &mut out {
            if crate::version::channel_of(&release.tag) != crate::version::NIGHTLY {
                continue;
            }
            if let Ok(Some(found)) = looked_up(Some(&release.tag), None) {
                release.channel = crate::version::channel_of(&found.id).to_string();
                release.id = found.id;
            }
        }
        Ok(out)
    }

    /// The feed's releases as it lists them, newest first, read once.
    fn feed() -> Result<Vec<Release>> {
        const FEED: &str = "https://api.github.com/repos/balaurengine/balaur/releases?per_page=30";
        let text =
            crate::runtimes::fetch_text(FEED)?.context("the release feed answered nothing")?;
        parse_feed(&text)
    }

    /// The feed's releases, in its order, without the network. A draft is not
    /// fetchable, so it is no row.
    fn parse_feed(text: &str) -> Result<Vec<Release>> {
        let feed: serde_json::Value = serde_json::from_str(text)?;
        let items = feed.as_array().context("the release feed is not a list")?;
        let mut out = Vec::new();
        for item in items {
            if item.get("draft").and_then(serde_json::Value::as_bool) == Some(true) {
                continue;
            }
            let Some(tag) = item.get("tag_name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            out.push(Release {
                channel: crate::version::channel_of(tag).to_string(),
                published: item
                    .get("published_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                id: tag.to_string(),
                tag: tag.to_string(),
            });
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
        let Some(text) = crate::runtimes::fetch_text(&format!("{}/VERSION", source.base()))? else {
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

    /// Why this install cannot replace itself, or None when it can. A screen
    /// asks before it offers the press; `install` refuses on the same answer.
    pub(crate) fn held() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let install = exe.parent()?;
        // The archive has the layout of a plain download, not a bundle's, and
        // the ticket stapled to the app covers what it would replace.
        if install.ends_with("Contents/MacOS") {
            return Some("Balaur.app updates by downloading the new app, not in place".into());
        }
        // A release unpacked over cargo's output buries what the build wrote.
        // No path: the line lands in screenshots, and the folder is the reader's.
        if build_tree(install) {
            return Some(
                "this runs from cargo's target directory, not an install; \
                 update a source build with git"
                    .into(),
            );
        }
        None
    }

    /// Where a held macOS bundle gets a release instead: its zipped app. Empty
    /// anywhere an install replaces itself.
    pub(crate) fn download_url(tag: &str) -> String {
        let bundled = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.ends_with("Contents/MacOS")))
            .unwrap_or(false);
        if bundled {
            format!("{RELEASE_BASE}/download/{tag}/balaur-editor-macos-universal.zip")
        } else {
            String::new()
        }
    }

    /// Replace this install with what a channel or a tag holds, telling
    /// `progress` how far it has got. Answers the line to show when it worked.
    pub(crate) fn replace(
        tag: Option<&str>,
        channel: Option<&str>,
        allow_downgrade: bool,
        progress: &mut dyn FnMut(Step),
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
        if let Some(why) = held() {
            bail!("{why}");
        }
        let exe = std::env::current_exe().context("locating the running executable")?;
        let install = exe
            .parent()
            .context("the executable has no parent directory")?
            .to_path_buf();
        let staged = download_and_unpack(&assets_base(&base, published), &install, progress)?;
        swap_install(&staged, &install, &exe)?;
        std::fs::remove_dir_all(&staged).ok();
        Ok(format!("updated to {published}; restart to use it"))
    }

    pub(crate) fn run(opts: &crate::UpdateOpts) -> Result<()> {
        let found = published(opts.version.as_deref(), opts.channel.as_deref())?;
        let own = crate::version::build_id().unwrap_or("dev");
        tracing::info!("installed: {own}; {}: {}", found.source, found.id);
        if opts.check {
            return Ok(());
        }
        let note = replace(
            opts.version.as_deref(),
            opts.channel.as_deref(),
            opts.allow_downgrade,
            &mut |_| {},
        )?;
        tracing::info!("{note}");
        Ok(())
    }

    /// Fetch the editor archive for this platform, verify it, and unpack it
    /// into a staging directory inside the install dir (same filesystem, so
    /// the swap is renames). Returns the unpacked bundle root.
    fn download_and_unpack(
        base: &str,
        install: &Path,
        progress: &mut dyn FnMut(Step),
    ) -> Result<PathBuf> {
        let target = host_target()?;
        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let name = format!("balaur-editor-{target}.{ext}");
        let url = format!("{base}/{name}");
        let expected = crate::runtimes::expected_sha256(&format!("{base}/SHA256SUMS"), &name)?;
        let staging = install.join(".balaur-update");
        std::fs::remove_dir_all(&staging).ok();
        std::fs::create_dir_all(&staging)?;
        let archive = staging.join(&name);
        crate::runtimes::download_reporting(
            &url,
            &archive,
            expected.as_deref(),
            &mut |done, total| {
                progress(Step::Downloading(done, total));
            },
        )?;
        progress(Step::Unpacking);
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
            assert!(base(None, Some("nightly")).ends_with("/download/nightly"));
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
            let stable = base(None, Some("stable"));
            let named = super::assets_base(&stable, "v0.3.0");
            assert!(named.ends_with("/download/v0.3.0"));
            let nightly = base(None, Some("nightly"));
            assert_eq!(super::assets_base(&nightly, "nightly-abc1234"), nightly);
        }

        #[test]
        fn the_feed_lists_releases_but_not_drafts() {
            let feed = r#"[
                {"tag_name": "nightly", "name": "Balaur nightly (716854d)", "draft": false, "published_at": "2026-09-17T18:00:00Z"},
                {"tag_name": "v0.2.0-alpha.3", "name": "Balaur v0.2.0-alpha.3", "draft": false, "published_at": "2026-09-16T09:00:00Z"},
                {"tag_name": "v0.2.0-alpha.2", "name": "Balaur v0.2.0-alpha.2", "draft": false, "published_at": "2026-09-15T09:00:00Z"},
                {"tag_name": "v0.2.0", "name": "Balaur v0.2.0", "draft": true, "published_at": null},
                {"tag_name": "v0.1.0", "name": "Balaur v0.1.0", "draft": false, "published_at": "2026-09-09T12:00:00Z"}
            ]"#;
            let rows = super::parse_feed(feed).expect("the feed parses");
            let tags: Vec<&str> = rows.iter().map(|r| r.tag.as_str()).collect();
            assert_eq!(
                tags,
                ["nightly", "v0.2.0-alpha.3", "v0.2.0-alpha.2", "v0.1.0"]
            );
            let lines: Vec<&str> = rows.iter().map(|r| r.channel.as_str()).collect();
            assert_eq!(lines, ["nightly", "alpha", "alpha", "stable"]);
            assert_eq!(
                super::newest_on(&rows, "alpha").as_deref(),
                Some("v0.2.0-alpha.3")
            );
            assert_eq!(super::newest_on(&rows, "beta"), None);
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
            std::fs::write(bundle.join("editor").join("a.bs"), b"x")
                .expect("the new editor file writes");

            super::swap_install(&bundle, &install, &install.join("balaur"))
                .expect("the swap succeeds");
            assert_eq!(super::read_all(&install.join("balaur")), b"new");
            assert!(install.join("editor").join("a.bs").is_file());
        }
    }
}
