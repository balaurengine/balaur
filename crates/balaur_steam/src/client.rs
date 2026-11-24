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
    CallbackMsg_t, GetTicketForWebApiResponse_t, HSteamPipe, SteamAPICall_t,
    SteamAPICallCompleted_t, SteamErrMsg, k_EResultOK, k_ESteamAPIInitResult_OK, zeroed,
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
        let mut api = unsafe { Api::load(library) }
            .map_err(|missing| format!("{} has no {missing}", path.display()))?;
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
    /// `pending` and the Web API tickets `platform.identity` waits on.
    pub(crate) fn dispatch(
        &self,
        pending: &mut Pending,
        tickets: &RefCell<VecDeque<Ticket>>,
        report: &Sender<Arrival>,
    ) {
        self.each_callback(|id, data| {
            if id == SteamAPICallCompleted_t::k_iCallback {
                if let Some((request, answer)) = self.completed(data, pending) {
                    let _ = report.send(Arrival::Answer { request, answer });
                }
                return;
            }
            if id == GetTicketForWebApiResponse_t::k_iCallback
                && let Some(ticket) = read::<GetTicketForWebApiResponse_t>(data)
            {
                tickets.borrow_mut().push_back(ticket_of(&ticket));
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
    fn completed(&self, data: &[u8], pending: &mut Pending) -> Option<(u64, serde_json::Value)> {
        let done = read::<SteamAPICallCompleted_t>(data)?;
        let handle = done.m_hAsyncCall;
        let (request, expected) = pending.remove(&handle)?;
        let mut buffer = vec![0_u8; usize::try_from(done.m_cubParam).unwrap_or(0)];
        let mut failed = false;
        // SAFETY: a buffer of the size Steam said the result takes.
        let got = unsafe {
            (self.api.globals.manual_dispatch_get_api_call_result)(
                self.pipe,
                handle,
                buffer.as_mut_ptr().cast(),
                i32::try_from(buffer.len()).unwrap_or(0),
                expected,
                &raw mut failed,
            )
        };
        let answer = match callback_value(expected, &buffer).filter(|_| got) {
            Some((kind, value)) => answer_json(kind, value, Some(failed)),
            None => failure(
                balaur_core::handler::ERROR,
                "Steam gave no result for the call",
            ),
        };
        Some((request, answer))
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
