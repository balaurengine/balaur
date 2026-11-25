//! Links into the game: `org.balaur.platform.Links` in the template's
//! `classes.dex` (`java/`).
//!
//! A link on one of `[android] schemes` lands on `org.balaur.platform.Redirect`,
//! which hands it to `WebSignIn` when it ends the open sign-in page and to
//! the shim otherwise, never both. The shim holds what lands until the game
//! listens, the first tick this runs, so the link that started the process
//! arrives too: each once, as `url_opened` with its `url`, an answer to
//! request 0 as a notification's tap is.

use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use jni::Env;
use jni::objects::{JClass, JObject};
use jni::refs::Global;

const CLASS: &str = "org.balaur.platform.Links";

/// The shim, loaded and its native method registered once.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

/// Whether the shim was told to hand its links over.
static LISTENING: AtomicBool = AtomicBool::new(false);

/// Tell the shim to hand over what landed, once the activity is there.
pub(crate) fn listen() {
    super::listen_once(&LISTENING, load);
}

fn load<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<&'cache Global<JClass<'static>>> {
    // Safety: `answer_native` points at a function of its own signature.
    unsafe { super::shim(env, activity, CLASS, &[super::answer_native()], &SHIM) }
}
