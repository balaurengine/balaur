//! Any program that compiles: calling each of its public functions, under the
//! fuel and memory limits, traps or answers and never brings the host down.
#![no_main]

use balaur_bs::{Options, compile};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(c) = compile(src, &Options::default()) else {
        return;
    };
    balaur_fuzz::call_every_public(&c);
});
