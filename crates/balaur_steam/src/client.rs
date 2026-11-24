//! The Steam client: Valve's library opened beside the game, started once,
//! and its callbacks taken by hand each tick (`SteamAPI_ManualDispatch_*`).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::mpsc::Sender;

use balaur_script::Value;
use serde::{Deserialize, Serialize};

use crate::library::Library;
use crate::marshal::read;
use crate::sdk::api::Api;
use crate::sdk::script::callback_value;
use crate::sdk::types::{
    CallbackMsg_t, EResult, GetTicketForWebApiResponse_t, HSteamPipe, SteamAPICall_t,
    SteamAPICallCompleted_t, SteamErrMsg, UserStatsReceived_t, UserStatsStored_t, k_EResultOK,
    k_ESteamAPIInitResult_OK, zeroed,
};

pub(crate) const KIND: &str = "kind";
pub(crate) const FAILED: &str = "failed";
pub(crate) const UNSUPPORTED: &str = "unsupported";

/// What Steam sent this tick, as a recording keeps it: a call result's
/// answer, or a callback for `steam_event`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Arrival {
    Answer {
        request: u64,
        answer: serde_json::Value,
    },
    Event {
        event: serde_json::Value,
    },
}

/// A Web API ticket as its callback delivered it, for `platform.identity`.
pub(crate) type Ticket = Result<Vec<u8>, String>;

/// What the backend behind `platform.*` waits on, filled by the steam
/// plugin's pump, which takes every callback, and read by the backend's.
#[derive(Default)]
pub(crate) struct Inbox {
    /// The call results the backend started, by Steam's handle: the callback
    /// id each answers with.
    pub(crate) calls: BTreeMap<SteamAPICall_t, i32>,
    /// What arrived for it, oldest first.
    pub(crate) arrived: VecDeque<Heard>,
}

/// One thing Steam told the backend.
pub(crate) enum Heard {
    Ticket(Ticket),
    /// `UserStatsReceived_t`: a player's stats and achievements are in, or
    /// failed to come.
    Stats {
        user: u64,
        result: EResult,
    },
    /// `UserStatsStored_t`, after a `StoreStats`.
    Stored {
        result: EResult,
    },
    /// A call result the backend started, its struct's bytes, or None when
    /// Steam gave none.
    Answer {
        handle: SteamAPICall_t,
        data: Option<Vec<u8>>,
    },
}

/// The call results in flight: Steam's handle, and the token and callback id
/// its answer goes to.
pub(crate) type Pending = BTreeMap<SteamAPICall_t, (u64, i32)>;

/// A started client. Dropping it shuts Steam down.
pub struct Steam {
    pub(crate) api: Api,
    pipe: HSteamPipe,
}

impl Steam {
    /// Valve's library beside the executable, started, or None, logged at
    /// info: no library there, or no Steam client running this game.
    pub(crate) fn start() -> Option<Self> {
        let path = crate::library::beside_executable()?;
        if !path.is_file() {
            tracing::info!(
                "steam: no {} beside the game; platform.* left as it was",
                crate::library::FILE
            );
            return None;
        }
        match Self::start_from(&path) {
            Ok(steam) => Some(steam),
            Err(why) => {
                tracing::info!(%why, "steam: not started; platform.* left as it was");
                None
            }
        }
    }

    /// Open the library at `path` and start Steam through it.
    ///
    /// # Errors
    /// The library will not open, lacks a function start-up needs, or
    /// `SteamAPI_InitFlat` refused: no client running, or none for this game.
    pub fn start_from(path: &Path) -> Result<Self, String> {
        // SAFETY: a file named as Valve names its library, beside the game.
        let library = unsafe { Library::open(path) }?;
        // SAFETY: the same library.
        unsafe { Self::start_with(library) }.map_err(|why| format!("{}: {why}", path.display()))
    }

    /// Start Steam through an open library.
    ///
    /// # Safety
    /// `library` is Valve's `steam_api`, or a test's functions of its types.
    pub(crate) unsafe fn start_with(library: Library) -> Result<Self, String> {
        // SAFETY: the caller's promise.
        let mut api = unsafe { Api::load(library) }.map_err(|missing| format!("no {missing}"))?;
        let mut message: SteamErrMsg = [0; 1024];
        // SAFETY: a buffer of the size Valve writes into.
        let result = unsafe { (api.globals.init_flat)(&raw mut message) };
        if result != k_ESteamAPIInitResult_OK {
            let text = match crate::marshal::chars(&message) {
                Value::Str(text) => text,
                _ => String::new(),
            };
            return Err(format!("SteamAPI_InitFlat answered {result}: {text}"));
        }
        // SAFETY: after a successful init, as Valve asks.
        unsafe { (api.globals.manual_dispatch_init)() };
        api.bind();
        // SAFETY: Steam has started.
        let pipe = unsafe { (api.globals.get_h_steam_pipe)() };
        tracing::info!("steam: the client runs this game");
        Ok(Self { api, pipe })
    }

    /// Valve's functions, for a game's own Rust.
    pub const fn api(&self) -> &Api {
        &self.api
    }

    /// Take every callback Steam has queued, answering the call results in
    /// `pending` and handing the backend what it waits on.
    pub(crate) fn dispatch(
        &self,
        pending: &mut Pending,
        inbox: &RefCell<Inbox>,
        report: &Sender<Arrival>,
    ) {
        self.each_callback(|id, data| {
            if id == SteamAPICallCompleted_t::k_iCallback {
                let Some(done) = read::<SteamAPICallCompleted_t>(data) else {
                    return;
                };
                if let Some((request, answer)) = self.completed(&done, pending) {
                    let _ = report.send(Arrival::Answer { request, answer });
                    return;
                }
                let mut inbox = inbox.borrow_mut();
                if let Some(expected) = inbox.calls.remove(&{ done.m_hAsyncCall }) {
                    let (data, got, failed) = self.result_of(&done, expected);
                    inbox.arrived.push_back(Heard::Answer {
                        handle: done.m_hAsyncCall,
                        data: (got && !failed).then_some(data),
                    });
                }
                return;
            }
            if let Some(heard) = heard(id, data) {
                inbox.borrow_mut().arrived.push_back(heard);
            }
            if let Some((kind, value)) = callback_value(id, data) {
                let _ = report.send(Arrival::Event {
                    event: answer_json(kind, value, None),
                });
            }
        });
    }

    /// Drop what Steam queued, on a tick a replay owns.
    pub(crate) fn discard(&self) {
        self.each_callback(|_, _| {});
    }

    fn each_callback(&self, mut each: impl FnMut(i32, &[u8])) {
        let globals = &self.api.globals;
        // SAFETY: Steam started and manual dispatch is on.
        unsafe { (globals.manual_dispatch_run_frame)(self.pipe) };
        let mut message: CallbackMsg_t = zeroed();
        // SAFETY: a message of Valve's layout to fill.
        while unsafe { (globals.manual_dispatch_get_next_callback)(self.pipe, &raw mut message) } {
            let (param, size) = (message.m_pubParam, message.m_cubParam);
            let data = match usize::try_from(size) {
                // SAFETY: Steam's buffer, valid until FreeLastCallback below.
                Ok(len) if !param.is_null() => unsafe { std::slice::from_raw_parts(param, len) },
                _ => &[],
            };
            each(message.m_iCallback, data);
            // SAFETY: after each message, as Valve asks.
            unsafe { (globals.manual_dispatch_free_last_callback)(self.pipe) };
        }
    }

    /// The answer to a call result Steam says is done, for the token it was
    /// asked under.
    fn completed(
        &self,
        done: &SteamAPICallCompleted_t,
        pending: &mut Pending,
    ) -> Option<(u64, serde_json::Value)> {
        let (request, expected) = pending.remove(&{ done.m_hAsyncCall })?;
        let (buffer, got, failed) = self.result_of(done, expected);
        let answer = match callback_value(expected, &buffer).filter(|_| got) {
            Some((kind, value)) => answer_json(kind, value, Some(failed)),
            None => failure(
                balaur_core::handler::ERROR,
                "Steam gave no result for the call",
            ),
        };
        Some((request, answer))
    }

    /// A finished call result's struct, as bytes: whether Steam gave it, and
    /// whether the call failed (`k_ESteamAPICallFailure*`).
    fn result_of(&self, done: &SteamAPICallCompleted_t, expected: i32) -> (Vec<u8>, bool, bool) {
        let mut buffer = vec![0_u8; usize::try_from(done.m_cubParam).unwrap_or(0)];
        let mut failed = false;
        // SAFETY: a buffer of the size Steam said the result takes.
        let got = unsafe {
            (self.api.globals.manual_dispatch_get_api_call_result)(
                self.pipe,
                done.m_hAsyncCall,
                buffer.as_mut_ptr().cast(),
                i32::try_from(buffer.len()).unwrap_or(0),
                expected,
                &raw mut failed,
            )
        };
        (buffer, got, failed)
    }

    /// The Web API ticket `platform.identity` asks for: with no identity,
    /// which is how a server checks it.
    pub(crate) fn ask_web_ticket(&self) -> bool {
        let user = &self.api.user;
        let (Some(ask), Some(this)) = (user.get_auth_ticket_for_web_api, user.this()) else {
            return false;
        };
        // SAFETY: Valve takes a null identity.
        unsafe { ask(this, std::ptr::null()) };
        true
    }
}

impl Drop for Steam {
    fn drop(&mut self) {
        // SAFETY: Steam started when this was built.
        unsafe { (self.api.globals.shutdown)() };
    }
}

/// What a callback tells the backend, if it waits on that kind.
fn heard(id: i32, data: &[u8]) -> Option<Heard> {
    match id {
        GetTicketForWebApiResponse_t::k_iCallback => read::<GetTicketForWebApiResponse_t>(data)
            .map(|ticket| Heard::Ticket(ticket_of(&ticket))),
        UserStatsReceived_t::k_iCallback => {
            read::<UserStatsReceived_t>(data).map(|got| Heard::Stats {
                user: { got.m_steamIDUser }.0,
                result: got.m_eResult,
            })
        }
        UserStatsStored_t::k_iCallback => {
            read::<UserStatsStored_t>(data).map(|stored| Heard::Stored {
                result: stored.m_eResult,
            })
        }
        _ => None,
    }
}

fn ticket_of(answer: &GetTicketForWebApiResponse_t) -> Ticket {
    let result = answer.m_eResult;
    if result != k_EResultOK {
        return Err(format!("Steam answered the ticket with EResult {result}"));
    }
    let bytes = answer.m_rgubTicket;
    let len = usize::try_from(answer.m_cubTicket)
        .unwrap_or(0)
        .min(bytes.len());
    Ok(bytes[..len].to_vec())
}

/// A callback's map: its `kind` first, then its fields.
fn answer_json(kind: &str, value: Value, failed: Option<bool>) -> serde_json::Value {
    let mut pairs = vec![(KIND.to_string(), Value::Str(kind.into()))];
    if let Some(failed) = failed {
        pairs.push((FAILED.into(), Value::Bool(failed)));
    }
    if let Value::Map(fields) = value {
        pairs.extend(fields);
    }
    balaur_core::engine_api::to_json(&Value::Map(pairs)).unwrap_or_default()
}

/// An answer that carries no result: `unsupported`, or `error`.
pub(crate) fn failure(kind: &str, why: &str) -> serde_json::Value {
    serde_json::json!({ KIND: kind, balaur_core::handler::ERROR: why })
}
