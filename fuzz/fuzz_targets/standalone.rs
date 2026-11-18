//! Any bytes as an executable: finding the pack a game carries answers or
//! says there is none, and a pack fused onto any bytes is found again.
#![no_main]

use balaur_core::standalone::{build, extract};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = extract(data);
    let (template, pack) = data.split_at(data.len() / 2);
    assert_eq!(
        extract(&build(template, pack)),
        Some(pack),
        "a fused pack was lost"
    );
});
