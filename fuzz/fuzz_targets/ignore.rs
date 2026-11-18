//! A `project.toml` up to the first zero byte, a path after it: reading its
//! ignore patterns and matching the path against each answers, in time.
#![no_main]

use balaur_core::ignore::{from_manifest, glob_matches, matching};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((manifest, path)) = balaur_fuzz::text_then_bytes(data) else {
        return;
    };
    let Ok(path) = std::str::from_utf8(path) else {
        return;
    };
    let patterns = from_manifest(manifest);
    let _ = matching(&patterns, path);
    // Any text as one pattern too, not only the ones a manifest lists.
    let _ = glob_matches(manifest, path);
});
