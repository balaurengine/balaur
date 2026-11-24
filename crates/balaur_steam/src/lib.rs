//! Steam as the store behind `platform.*`: who is playing
//! (`platform.sign_in`) and the Web API ticket a server checks with Steam's
//! `ISteamUserAuth/AuthenticateUserTicket` (`platform.identity`).
//!
//! Only where the Steam client runs the game, on Windows, macOS or Linux.
//! With no client, or anywhere else, the plugin loads and does nothing:
//! `platform.*` stays with whatever backend filled it (Apple's on a Mac), or
//! none, and a script runs unchanged. It registers after Apple's, so a game
//! started by Steam on a Mac answers as Steam.
//!
//! A Steam template ships Valve's library beside it, and the export copies it
//! beside the game; achievements, boards and cloud saves are
//! `docs/PLAN-steam.md` steps 3 on.

use anyhow::Result;

const STEAM: &str = "steam";

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod client;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod client {
    /// No Steam client exists here.
    #[allow(clippy::unnecessary_wraps, reason = "the desktop one can fail")]
    pub(crate) const fn declare(_reg: &mut balaur_plugin::Registry<'_>) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The Steam backend, loaded where the client answers.
#[derive(Default)]
pub struct SteamPlugin;

impl balaur_plugin::Plugin for SteamPlugin {
    fn name(&self) -> &'static str {
        STEAM
    }

    fn requires(&self) -> Vec<String> {
        vec!["platform".to_string()]
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        client::declare(reg)
    }
}

/// A ticket's bytes as the hex Steam's Web API reads.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02X}");
            out
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_ticket_reads_as_upper_case_hex() {
        assert_eq!(super::hex(&[0x14, 0x00, 0xAB]), "1400AB");
    }
}
