//! Which build this binary is: a tagged release, a nightly, or built from
//! source. `scripts/package.sh` bakes the id in; a cargo build has none.

/// The line a build with no version of its own follows.
pub(crate) const NIGHTLY: &str = "nightly";
/// The line a version with no prerelease suffix is on.
pub(crate) const STABLE: &str = "stable";
/// Every name `--channel` takes. The first `PRERELEASE_LINES` are SemVer
/// prerelease identifiers, in the order SemVer puts them.
pub(crate) const CHANNELS: [&str; 5] = ["alpha", "beta", "rc", STABLE, NIGHTLY];
const PRERELEASE_LINES: usize = 3;

/// `v0.1.0` for a tagged release, `nightly-<sha>` for a nightly, `None` for
/// a source build.
pub(crate) fn build_id() -> Option<&'static str> {
    option_env!("BALAUR_BUILD")
}

/// What `--version` prints: the crate version plus the build id. Leaked
/// once, because clap keeps a `'static` str.
pub(crate) fn long() -> &'static str {
    let long = format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        build_id().unwrap_or("source build")
    );
    Box::leak(long.into_boxed_str())
}

/// The channel a build id names: the prerelease identifier of
/// `v0.2.0-alpha.3` is `alpha`, a version with no suffix is stable, and an
/// id that is no version tag at all is the rolling nightly.
pub(crate) fn channel_of(id: &str) -> &str {
    let Some(version) = id.strip_prefix('v') else {
        return NIGHTLY;
    };
    let version = version.split_once('+').map_or(version, |(v, _)| v);
    match version.split_once('-') {
        Some((_, pre)) => pre
            .split('.')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(STABLE),
        None => STABLE,
    }
}

/// The channel this build is on, or None for a source build.
pub(crate) fn channel() -> Option<&'static str> {
    build_id().map(channel_of)
}

/// The release tag this build's assets live under: a tagged build is its own
/// tag, a nightly follows the rolling `nightly` tag, a source build has none.
pub(crate) fn release_tag() -> Option<&'static str> {
    build_id().map(|id| {
        if channel_of(id) == NIGHTLY {
            NIGHTLY
        } else {
            id
        }
    })
}

/// Whether moving from `installed` to `published` is a step back. A nightly
/// and a source build order against nothing, so neither is ever a downgrade.
pub(crate) fn is_downgrade(published: &str, installed: &str) -> bool {
    match (precedence(published), precedence(installed)) {
        (Some(new), Some(own)) => new < own,
        _ => false,
    }
}

/// Sort key for a version tag: the three numbers, then the prerelease rank
/// (`alpha` < `beta` < `rc` < the release itself) and its count.
fn precedence(id: &str) -> Option<(u32, u32, u32, usize, u32)> {
    let version = id.strip_prefix('v')?;
    let (version, pre) = match version.split_once('-') {
        Some((version, pre)) => (version, Some(pre)),
        None => (version, None),
    };
    let mut parts = version.split('.');
    let mut number = || parts.next()?.parse::<u32>().ok();
    let (major, minor, patch) = (number()?, number()?, number()?);
    if parts.next().is_some() {
        return None;
    }
    let (rank, count) = match pre {
        None => (PRERELEASE_LINES, 0),
        Some(pre) => {
            let (name, count) = pre.split_once('.')?;
            let rank = CHANNELS[..PRERELEASE_LINES]
                .iter()
                .position(|line| *line == name)?;
            (rank, count.parse().ok()?)
        }
    };
    Some((major, minor, patch, rank, count))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_source_build_prints_that_it_is_one() {
        // Tests run from cargo, where package.sh has baked nothing in.
        assert!(super::long().ends_with("(source build)"));
        assert_eq!(super::release_tag(), None);
        assert_eq!(super::channel(), None);
    }

    #[test]
    fn a_version_names_the_channel_it_is_on() {
        assert_eq!(super::channel_of("v0.2.0-alpha.3"), "alpha");
        assert_eq!(super::channel_of("v0.2.0-rc.1"), "rc");
        assert_eq!(super::channel_of("v0.2.0"), super::STABLE);
        assert_eq!(super::channel_of("nightly-abc1234"), super::NIGHTLY);
    }

    #[test]
    fn a_prerelease_comes_before_the_release_it_leads_to() {
        assert!(super::is_downgrade("v0.2.0-alpha.3", "v0.2.0"));
        assert!(super::is_downgrade("v0.2.0-beta.1", "v0.2.0-rc.1"));
        assert!(super::is_downgrade("v0.1.0", "v0.2.0-alpha.1"));
        assert!(!super::is_downgrade("v0.2.0", "v0.2.0-alpha.3"));
        assert!(!super::is_downgrade("v0.2.0-alpha.4", "v0.2.0-alpha.3"));
    }

    #[test]
    fn a_build_with_no_version_orders_against_nothing() {
        assert!(!super::is_downgrade("nightly-abc1234", "v9.9.9"));
        assert!(!super::is_downgrade("v0.1.0", "nightly-abc1234"));
        assert!(!super::is_downgrade("v0.1.0", "dev"));
    }
}
