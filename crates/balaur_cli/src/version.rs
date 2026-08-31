//! Which build this binary is: a tagged release, a nightly, or built from
//! source. `scripts/package.sh` bakes the id in; a cargo build has none.

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

/// The release tag this build's assets live under: a tagged build is its own
/// tag, a nightly follows the rolling `nightly` tag, a source build has none.
pub(crate) fn release_tag() -> Option<&'static str> {
    build_id().map(|id| if id.starts_with('v') { id } else { "nightly" })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_source_build_prints_that_it_is_one() {
        // Tests run from cargo, where package.sh has baked nothing in.
        assert!(super::long().ends_with("(source build)"));
        assert_eq!(super::release_tag(), None);
    }
}
