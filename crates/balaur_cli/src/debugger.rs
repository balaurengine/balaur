//! `--debug`: the debug adapter a run serves, where a web build has none.

use anyhow::Result;
use balaur::App;

/// How long `--debug-wait` holds the boot for a client. Long enough to start
/// one by hand, short enough that a forgotten flag in CI fails rather than
/// hangs.
const DEBUG_ATTACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Serve the debug adapter for `--debug`, held open for the run.
///
/// # Errors
/// If the port cannot be bound, or no client attaches under `--debug-wait`.
#[cfg(not(target_family = "wasm"))]
pub(crate) fn start_debugger(
    app: &mut App,
    port: Option<u16>,
    wait: bool,
) -> Result<Option<balaur::dap::Server>> {
    let Some(port) = port else {
        return Ok(None);
    };
    let server = balaur::dap::serve(app, port)?;
    println!("debug adapter listening on {}", server.addr());
    if wait {
        println!("waiting for a debugger to attach");
        server.wait_for_attach(DEBUG_ATTACH_TIMEOUT)?;
    }
    Ok(Some(server))
}

/// The adapter speaks over a TCP listener, which a web build has none of, so
/// `--debug` is refused there rather than quietly doing nothing.
///
/// # Errors
/// If `--debug` was given.
#[cfg(target_family = "wasm")]
pub(crate) fn start_debugger(_app: &mut App, port: Option<u16>, _wait: bool) -> Result<Option<()>> {
    anyhow::ensure!(
        port.is_none(),
        "--debug needs a TCP listener, and a web build has none"
    );
    Ok(None)
}
