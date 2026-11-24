//! Steam for Balaur: Valve's flat API, loaded at run time from the
//! `steam_api` beside the game, behind `platform.*` and the `steam_*` script
//! modules.
//!
//! Every desktop build has it. With no library beside the executable, or no
//! Steam client running this game, the plugin loads and does nothing:
//! `platform.*` stays with whatever backend filled it (Apple's on a Mac), and
//! every `steam_*` call answers nil, or `unsupported` for one that answers
//! later. It registers after Apple's, so a game started by Steam on a Mac
//! answers as Steam. `SteamAPI_RestartAppIfNecessary` is never called.
//!
//! [`sdk`] is Valve's API whole, generated from the SDK's `steam_api.json` by
//! `scripts/gen_steam.py`; `sdk/unmapped.md` lists what the script modules
//! leave out and why.

use std::rc::Rc;

use anyhow::Result;
use balaur_core::{Engine, Stage};
use balaur_script::{Module, Value};

mod answers;
mod client;
#[cfg(all(test, not(target_family = "wasm")))]
mod fake;
pub mod library;
mod marshal;
mod platform;
#[cfg(all(test, not(target_family = "wasm")))]
mod platform_tests;
pub mod sdk;

pub use answers::{SteamSnapshot, SteamState};
pub use client::Steam;

const STEAM: &str = "steam";
const IS_LOADED: &str = "is_loaded";
const REQUEST: &str = "request";
const ARRIVALS: &str = "steam";
/// The event every Valve callback is, its `kind` the callback's name.
pub const STEAM_EVENT: &str = "steam_event";

/// The Steam backend, and the `steam` script modules.
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
        reg.insert_resource(SteamState::default());
        reg.insert_resource(SteamSnapshot::default());
        reg.add_system(Stage::First, answers::pump_steam_system);
        reg.add_replay_source(
            ARRIVALS,
            answers::capture_arrivals,
            answers::restore_arrivals,
        );
        reg.add_replay_source(answers::ANSWERS, answers::capture, answers::restore);
        // The header carries what `init` asked, before the first recorded tick.
        reg.add_replay_setup(answers::ANSWERS, answers::capture, answers::restore);
        balaur_core::events::declare(
            reg.engine(),
            &[(
                STEAM_EVENT,
                "a Steam callback",
                "Steam said something: the map's `kind` is the callback's name, a `steam::EVENT_*` constant, beside its fields.",
            )],
        );
        for &(name, install) in sdk::script::MODULES {
            let mut m = reg.script_module(name)?;
            install(&mut m);
            if name == STEAM {
                install_steam(&mut m);
            }
        }
        if let Some(steam) = client::Steam::start() {
            install(reg.engine(), steam)?;
        }
        Ok(())
    }
}

/// A started client behind the `steam_*` modules and `platform.*`.
fn install(eng: &Engine, steam: Steam) -> Result<()> {
    let steam = Rc::new(steam);
    let state = eng.resource::<SteamState>();
    let inbox = Rc::clone(&state.borrow().inbox);
    state.borrow_mut().steam = Some(Rc::clone(&steam));
    let backend = platform::SteamBackend::new(steam, inbox);
    balaur_platform::set_backend(eng, Box::new(backend))
}

/// What the `steam` module adds to Valve's own: whether Steam is here.
fn install_steam(m: &mut Module<Engine>) {
    balaur_core::handler::install_event_kinds(m, sdk::script::KINDS);
    balaur_core::handler::install_event_kinds(
        m,
        &[client::UNSUPPORTED, balaur_core::handler::ERROR],
    );
    m.describe(&[(
        IS_LOADED,
        &[],
        "() -> bool",
        "Whether Valve's `steam_api` is beside the game and the Steam client runs it. When false every `steam_*` call answers nil.",
    )]);
    m.function_raw(
        IS_LOADED,
        Box::new(|eng: &Engine, _: &[Value]| {
            let loaded = answers::answer(eng, |_| Some(Value::Bool(true)));
            Ok(Value::Bool(loaded == Value::Bool(true)))
        }),
    );
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
    use std::cell::RefCell;
    use std::rc::Rc;

    use balaur_core::{App, AppConfig, Engine};
    use balaur_platform::{PlatformPlugin, PlatformState};
    use balaur_script::{Call, Declared, Module, Value};

    use crate::sdk::types::{GetTicketForWebApiResponse_t, LobbyEnter_t};
    use crate::{SteamPlugin, SteamSnapshot};

    fn app() -> App {
        let mut app = App::new(AppConfig::bare(".")).unwrap();
        balaur_plugin::load(&mut app, &mut PlatformPlugin).unwrap();
        balaur_plugin::load(&mut app, &mut SteamPlugin).unwrap();
        app
    }

    /// Every module the plugin declares, as a script host would hold them.
    fn declared() -> Rc<RefCell<Declared<Engine>>> {
        let declared = Rc::new(RefCell::new(Declared::default()));
        for &(name, install) in crate::sdk::script::MODULES {
            let mut m = Module::new(name, Rc::clone(&declared));
            install(&mut m);
            if name == crate::STEAM {
                crate::install_steam(&mut m);
            }
        }
        declared
    }

    fn call(
        eng: &Engine,
        declared: &Declared<Engine>,
        module: &str,
        name: &str,
        args: &[Value],
    ) -> Value {
        let entry = &declared.modules[module][name];
        let Some(Call::Raw(f)) = &entry.call else {
            panic!("{module}::{name} is not registered raw");
        };
        f(eng, args).unwrap()
    }

    #[test]
    fn a_ticket_reads_as_upper_case_hex() {
        assert_eq!(super::hex(&[0x14, 0x00, 0xAB]), "1400AB");
    }

    #[test]
    fn with_no_library_every_call_answers_nil_and_platform_is_left_alone() {
        let app = app();
        assert_eq!(
            app.engine
                .resource::<PlatformState>()
                .borrow()
                .backend_name(),
            "none"
        );
        let declared = declared();
        let declared = declared.borrow();
        let eng = &app.engine;
        assert_eq!(
            call(eng, &declared, "steam", "is_loaded", &[]),
            Value::Bool(false)
        );
        assert_eq!(
            call(eng, &declared, "steam", "is_steam_running", &[]),
            Value::Nil
        );
        assert_eq!(
            call(eng, &declared, "steam_user", "get_steam_id", &[]),
            Value::Nil
        );
        assert_eq!(
            call(eng, &declared, "steam_friends", "get_persona_name", &[]),
            Value::Nil
        );
        assert_eq!(
            call(
                eng,
                &declared,
                "steam_apps",
                "get_current_game_language",
                &[]
            ),
            Value::Nil
        );
        assert_eq!(
            call(
                eng,
                &declared,
                "steam_user_stats",
                "get_achievement",
                &[Value::Str("WIN".into())]
            ),
            Value::Nil
        );
        let identity = Value::Map(vec![("type".into(), Value::Int(16))]);
        assert_eq!(
            call(
                eng,
                &declared,
                "steam_networking_identity",
                "clear",
                &[identity]
            ),
            Value::Nil,
            "a struct's own functions are the library's too"
        );
    }

    #[test]
    fn a_wrong_argument_still_fails_without_the_library() {
        let app = app();
        let declared = declared();
        let declared = declared.borrow();
        let entry = &declared.modules["steam_user_stats"]["get_achievement"];
        let Some(Call::Raw(f)) = &entry.call else {
            panic!("not raw");
        };
        let why = f(&app.engine, &[Value::Int(3)]).unwrap_err();
        assert!(format!("{why}").contains("name"), "{why}");
    }

    #[test]
    fn a_call_result_without_steam_answers_unsupported_on_the_next_tick() {
        let mut app = app();
        let declared = declared();
        let id = call(
            &app.engine,
            &declared.borrow(),
            "steam_user",
            "request_encrypted_app_ticket",
            &[Value::List(vec![])],
        );
        let Value::Int(id) = id else {
            panic!("a call result answers its id, got {id:?}");
        };
        app.tick(1.0 / 60.0);
        let answers = app
            .engine
            .resource::<SteamSnapshot>()
            .borrow()
            .answers
            .clone();
        let Some(Value::Map(pairs)) = answers.first() else {
            panic!("no answer in {answers:?}");
        };
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(get("kind"), Some(Value::Str("unsupported".into())));
        assert_eq!(get("request"), Some(Value::Int(id)));
    }

    #[test]
    fn every_function_has_its_docs_and_every_signature_reads() {
        let declared = declared();
        let declared = declared.borrow();
        let mut count = 0;
        for (module, entries) in &declared.modules {
            assert!(
                declared.module_docs.contains_key(module),
                "{module} has no doc"
            );
            for (name, entry) in entries {
                assert!(entry.doc.is_some(), "{module}::{name} has no doc");
                assert!(
                    entry.documented.is_some(),
                    "{module}::{name} has no signature"
                );
                let signature = entry.documented.unwrap();
                assert!(
                    !(signature.contains(": any") || signature.contains("-> any")),
                    "{module}::{name} is untyped: {signature}"
                );
                count += 1;
            }
        }
        assert!(count > 700, "{count} functions");
    }

    // The one callback the engine reads itself, against steamworks-sys 0.13.0's
    // bindgen record on every platform, and one whose 64-bit field packs to 8 on
    // Windows and 4 elsewhere.
    #[test]
    fn callback_layouts_match_steamworks_sys_on_every_platform() {
        let layouts: serde_json::Value =
            serde_json::from_str(include_str!("../sdk/layouts.json")).unwrap();
        for platform in ["linux64", "linuxarm64", "macos", "windows"] {
            let ticket = &layouts[platform]["GetTicketForWebApiResponse_t"];
            assert_eq!(ticket["size"], 2572, "{platform}");
            assert_eq!(ticket["offsets"]["m_rgubTicket"], 12, "{platform}");
        }
        assert_eq!(layouts["linux64"]["LobbyEnter_t"]["size"], 20);
        assert_eq!(layouts["windows"]["LobbyEnter_t"]["size"], 24);
        let host = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_arch = "aarch64") {
            "linuxarm64"
        } else {
            "linux64"
        };
        let ticket = &layouts[host]["GetTicketForWebApiResponse_t"];
        assert_eq!(
            size_of::<GetTicketForWebApiResponse_t>(),
            ticket["size"].as_u64().unwrap() as usize
        );
        for (field, offset) in [
            (
                "m_hAuthTicket",
                std::mem::offset_of!(GetTicketForWebApiResponse_t, m_hAuthTicket),
            ),
            (
                "m_eResult",
                std::mem::offset_of!(GetTicketForWebApiResponse_t, m_eResult),
            ),
            (
                "m_cubTicket",
                std::mem::offset_of!(GetTicketForWebApiResponse_t, m_cubTicket),
            ),
            (
                "m_rgubTicket",
                std::mem::offset_of!(GetTicketForWebApiResponse_t, m_rgubTicket),
            ),
        ] {
            assert_eq!(
                ticket["offsets"][field].as_u64().unwrap() as usize,
                offset,
                "{field}"
            );
        }
        let lobby = &layouts[host]["LobbyEnter_t"];
        assert_eq!(
            size_of::<LobbyEnter_t>(),
            lobby["size"].as_u64().unwrap() as usize
        );
        assert_eq!(GetTicketForWebApiResponse_t::k_iCallback, 168);
    }
}
