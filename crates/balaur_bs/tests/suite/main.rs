//! Every integration test this crate has, in one binary.

mod anys;
mod awaits;
mod budgets;
mod classes;
mod debugger;
mod determinism;
mod direct;
mod engine_api;
mod fnrefs;
mod format;
#[cfg(all(feature = "c", feature = "native"))]
mod freestanding;
mod library;
mod loops;
mod maps;
mod memory;
mod modules;
mod nans;
#[cfg(all(feature = "c", feature = "native"))]
mod native;
mod optional;
mod profile;
mod reentry;
mod refs;
mod reload;
mod returns;
mod sandbox;
mod syntax;
mod tools;
mod units;

use balaur_bs::runtime::{HostFns, Limits, Runtime, Value};
use balaur_bs::{Options, compile};

pub(crate) fn run(src: &str) -> Runtime {
    run_with(src, HostFns::default(), &Options::default())
}

pub(crate) fn run_with(src: &str, host: HostFns, options: &Options) -> Runtime {
    let c = compile(src, options).unwrap_or_else(|e| panic!("{e}\n{src}"));
    Runtime::new(&c, host, Limits::default()).unwrap_or_else(|e| panic!("{e}"))
}

pub(crate) fn call(rt: &mut Runtime, f: &str, args: &[Value]) -> Value {
    rt.call(f, args).unwrap_or_else(|e| panic!("{f}: {e}"))
}

pub(crate) fn compile_error(src: &str) -> String {
    compile(src, &Options::default())
        .err()
        .map_or_else(|| panic!("compiled:\n{src}"), |e| e.to_string())
}
