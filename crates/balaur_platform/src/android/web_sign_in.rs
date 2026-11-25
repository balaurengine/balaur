//! A sign-in page in a Custom Tab: `org.balaur.platform.WebSignIn` in the
//! template's `classes.dex` (`java/`), reached over JNI.
//!
//! The Java side answers on the UI thread, with the URL a page ended on (its
//! redirect reaches `org.balaur.platform.Redirect`, an intent filter of
//! `[android] schemes`), or `cancelled` when the game's activity comes back
//! with none. The answer crosses back on the channel the call started with.
//! A link on those schemes that ends no page is a link into the game
//! (`links.rs`), never also this page's answer.

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock, PoisonError};

use jni::objects::{JClass, JObject, JString, JValue};
use jni::refs::Global;
use jni::sys::jlong;
use jni::{Env, EnvUnowned, NativeMethod, jni_sig, jni_str};

use crate::{PlatformEvent, kind};

const CLASS: &str = "org.balaur.platform.WebSignIn";

/// Where an answer goes: the channel of the last page opened.
static REPORT: Mutex<Option<Sender<PlatformEvent>>> = Mutex::new(None);

/// The shim, loaded and its native method registered once.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

/// Show `url` in a Custom Tab, answering under `request` once it shuts.
pub(crate) fn open(request: u64, url: &str, scheme: Option<&str>, report: &Sender<PlatformEvent>) {
    // copy: the channel outlives this call, until the answer crosses it.
    *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(report.clone());
    let started = super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let url = env.new_string(url)?;
        let scheme = match scheme {
            Some(scheme) => JObject::from(env.new_string(scheme)?),
            None => JObject::null(),
        };
        env.call_static_method(
            shim,
            jni_str!("start"),
            jni_sig!("(Landroid/app/Activity;JLjava/lang/String;Ljava/lang/String;)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
                JValue::Object(&url),
                JValue::Object(&scheme),
            ],
        )?;
        Ok(())
    });
    if let Err(err) = started {
        let _ = report.send(PlatformEvent::Failed {
            request,
            message: format!("opening the sign-in page: {err}"),
        });
    }
}

/// Shut the open tab: the game's activity comes back to the front, and the
/// page answers `cancelled`. Nothing when no page was ever opened.
pub(crate) fn close() {
    if SHIM.get().is_none() {
        return;
    }
    let closed = super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            jni_str!("close"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(activity)],
        )?;
        Ok(())
    });
    // A close that failed leaves the page up, which the player can still shut.
    if let Err(err) = closed {
        tracing::warn!("platform: closing the sign-in page: {err}");
    }
}

fn load<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<&'cache Global<JClass<'static>>> {
    // Safety: `answer` takes the JNI environment, the class (a static
    // method), a long and two strings, as its signature says.
    unsafe {
        super::shim(
            env,
            activity,
            CLASS,
            &[NativeMethod::from_raw_parts(
                jni_str!("answer"),
                jni_str!("(JLjava/lang/String;Ljava/lang/String;)V"),
                answer as *mut c_void,
            )],
            &SHIM,
        )
    }
}

/// `WebSignIn.answer(long, String, String)`: a request's `kind`, and the URL
/// it ended on or the error's message.
extern "system" fn answer<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    request: jlong,
    answered: JString<'local>,
    detail: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let answered = answered.try_to_string(env)?;
        let detail = detail.try_to_string(env)?;
        let request = u64::try_from(request).unwrap_or_default();
        let event = match answered.as_str() {
            kind::WEB_SIGN_IN => PlatformEvent::WebSignIn {
                request,
                url: detail,
            },
            kind::CANCELLED => PlatformEvent::Cancelled { request },
            _ => PlatformEvent::Failed {
                request,
                message: detail,
            },
        };
        if let Some(report) = REPORT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _ = report.send(event);
        }
        Ok(())
    })
    .resolve::<jni::errors::LogErrorAndDefault>();
}
