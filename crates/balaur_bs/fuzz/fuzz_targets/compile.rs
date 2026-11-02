//! Any text: reading, formatting, analysing and compiling it never panics,
//! and every module the compiler writes validates.
#![no_main]

use balaur_bs::{HostApi, Options, analysis, compile, errors, fmt};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let _ = errors(src, &Options::default());
    let _ = fmt::format(src);
    let _ = analysis::analyze(src, &HostApi::default());
    if let Ok(c) = compile(src, &Options::default()) {
        if let Err(e) = wasmparser::Validator::new().validate_all(&c.wasm) {
            panic!("the compiler wrote a module that does not validate: {e}");
        }
    }
});
