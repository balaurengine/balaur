//! Steam as the store behind `platform.*`: who is playing, and the Web API
//! ticket a server checks with Steam.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use balaur_platform::{Call, PlatformBackend, PlatformEvent, Player};

use crate::client::{Steam, Ticket};

pub(crate) struct SteamBackend {
    pub(crate) steam: Rc<Steam>,
    /// Filled by the steam plugin's own pump, which takes Steam's callbacks.
    pub(crate) tickets: Rc<RefCell<VecDeque<Ticket>>>,
    /// The `identity` calls waiting for a ticket, oldest first: Steam answers
    /// them in the order they were asked.
    pub(crate) waiting: VecDeque<u64>,
}

impl SteamBackend {
    fn player(&self) -> Option<Player> {
        let api = &self.steam.api;
        let (id, this) = (api.user.get_steam_id?, api.user.this()?);
        let (name, friends) = (api.friends.get_persona_name?, api.friends.this()?);
        // SAFETY: both interfaces bound after Steam started.
        let id = unsafe { id(this) };
        // SAFETY: Steam keeps the name alive until the next call.
        let alias = match unsafe { crate::marshal::c_str_ptr(name(friends)) } {
            balaur_script::Value::Str(alias) => alias,
            _ => String::new(),
        };
        Some(Player {
            id: id.to_string(),
            alias,
        })
    }
}

impl PlatformBackend for SteamBackend {
    fn name(&self) -> &'static str {
        super::STEAM
    }

    fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
        let event = match call {
            Call::SignIn => match self.player() {
                Some(player) => PlatformEvent::SignedIn { request, player },
                None => PlatformEvent::Failed {
                    request,
                    message: "this steam_api has no ISteamUser or ISteamFriends".to_string(),
                },
            },
            Call::Identity if self.steam.ask_web_ticket() => {
                self.waiting.push_back(request);
                return;
            }
            other => PlatformEvent::Unsupported {
                request,
                call: other.name().to_string(),
            },
        };
        let _ = report.send(event);
    }

    fn pump(&mut self, report: &Sender<PlatformEvent>) {
        while let Some(ticket) = self.tickets.borrow_mut().pop_front() {
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
        self.tickets.borrow_mut().clear();
        self.waiting.clear();
    }
}
