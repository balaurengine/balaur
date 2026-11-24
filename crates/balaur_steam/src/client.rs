//! The Steam client: started once, its callbacks run each tick by the
//! platform pump, and the two calls `platform.*` asks of it.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyhow::Result;
use balaur_platform::{Call, PlatformBackend, PlatformEvent, Player};
use steamworks::{CallbackHandle, Client, TicketForWebApiResponse};

/// Start the client and take over `platform.*`, or leave it be where no
/// Steam client runs this game.
pub(crate) fn declare(reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
    let client = match Client::init() {
        Ok(client) => client,
        Err(error) => {
            tracing::info!(%error, "steam: no client runs this game; platform.* left as it was");
            return Ok(());
        }
    };
    tracing::info!(
        app = client.utils().app_id().0,
        "steam: the client runs this game"
    );
    let backend = SteamBackend::new(client);
    balaur_platform::set_backend(reg.engine(), Box::new(backend))
}

/// A ticket as Steam's callback delivered it.
type Ticket = std::result::Result<Vec<u8>, String>;

struct SteamBackend {
    client: Client,
    tickets: Receiver<Ticket>,
    /// The `identity` calls waiting for a ticket, oldest first: Steam answers
    /// them in the order they were asked.
    waiting: VecDeque<u64>,
    _on_ticket: CallbackHandle,
}

impl SteamBackend {
    fn new(client: Client) -> Self {
        // Read only by `pump`, which the platform runs inside
        // `ExternalIo::start`, so a replay never takes a ticket from it.
        let (sent, tickets): (Sender<Ticket>, Receiver<Ticket>) = channel();
        let on_ticket = client.register_callback(move |answer: TicketForWebApiResponse| {
            let ticket = match answer.result {
                Ok(()) => Ok(answer.ticket),
                Err(error) => Err(error.to_string()),
            };
            let _ = sent.send(ticket);
        });
        Self {
            client,
            tickets,
            waiting: VecDeque::new(),
            _on_ticket: on_ticket,
        }
    }

    /// Ask for a Web API ticket with no identity, which is how the server
    /// checks it (`AuthenticateUserTicket` without one).
    fn ask_ticket(&mut self, request: u64) {
        // The safe binding takes an identity string; the server sends none,
        // so the ticket is asked for with none.
        unsafe {
            let user = steamworks::sys::SteamAPI_SteamUser_v023();
            steamworks::sys::SteamAPI_ISteamUser_GetAuthTicketForWebApi(user, std::ptr::null());
        }
        self.waiting.push_back(request);
    }
}

impl PlatformBackend for SteamBackend {
    fn name(&self) -> &'static str {
        super::STEAM
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        match call {
            Call::SignIn => {
                let player = Player {
                    id: self.client.user().steam_id().raw().to_string(),
                    alias: self.client.friends().name(),
                };
                let _ = report.send(PlatformEvent::SignedIn { request, player });
            }
            Call::Identity => self.ask_ticket(request),
            other => {
                let _ = report.send(PlatformEvent::Unsupported {
                    request,
                    call: other.name().to_string(),
                });
            }
        }
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        self.client.run_callbacks();
        while let Ok(ticket) = self.tickets.try_recv() {
            let Some(request) = self.waiting.pop_front() else {
                continue;
            };
            let event = match ticket {
                Ok(bytes) => PlatformEvent::Identity {
                    request,
                    proof: serde_json::json!({ "ticket": super::hex(&bytes) }),
                },
                Err(message) => PlatformEvent::Failed { request, message },
            };
            let _ = report.send(event);
        }
    }

    fn discard(&mut self) {
        self.client.run_callbacks();
        while self.tickets.try_recv().is_ok() {}
        self.waiting.clear();
    }
}
