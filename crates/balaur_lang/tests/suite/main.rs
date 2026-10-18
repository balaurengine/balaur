//! Every integration test this crate has, in one binary.

mod awaits;
mod debugger;
mod determinism;
mod library;
mod loops;
mod memory;
mod reload;
mod returns;
mod sandbox;
mod syntax;

use balaur_lang::runtime::{HostFns, Limits, Runtime, Value};
use balaur_lang::{Options, compile};

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
