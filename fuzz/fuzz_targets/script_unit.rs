//! A compiled unit as a pack carries it, damaged: a program up to the first
//! zero byte, compiled and written out, then the rest as patches over every
//! byte after the compiler's name. Reading it back refuses or yields a unit
//! whose functions trap or answer; nothing brings the host down.
#![no_main]

use balaur_bs::{Compiled, Options, compile};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((src, patches)) = balaur_fuzz::text_then_bytes(data) else {
        return;
    };
    let Ok(c) = compile(src, &Options::default()) else {
        return;
    };
    let bytes = c.to_bytes();
    // The compiler's name leads, as a length and its bytes: a unit read by
    // another compiler is refused there, which no patch should spend time on.
    let named = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let bytes = balaur_fuzz::patched(bytes, 4 + named, patches);
    if let Some(unit) = Compiled::from_bytes(&bytes) {
        balaur_fuzz::call_every_public(&unit);
    }
});
