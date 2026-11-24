//! Android: `org.balaur.google.SignIn` in the template's `classes.dex`
//! (`java/`), reached over JNI through `balaur_platform`'s activity and shim
//! loader. The Java side answers on the UI thread; the answer crosses back to
//! the game on the channel the call started with.

use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock, PoisonError};

use jni::objects::{JClass, JString, JValue};
use jni::refs::Global;
use jni::sys::jlong;
use jni::{EnvUnowned, NativeMethod, jni_sig, jni_str};

use crate::GoogleEvent;

/// Where an answer goes: the channel of the last call started, which every
/// call shares.
static REPORT: Mutex<Option<Sender<GoogleEvent>>> = Mutex::new(None);

/// The shim, loaded and its native method registered once.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

pub(crate) fn sign_in(request: u64, server_client_id: &str, report: &Sender<GoogleEvent>) {
    // copy: the channel outlives this call, until the answer crosses it.
    *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(report.clone());
    if let Err(err) = start(request, server_client_id) {
        let _ = report.send(GoogleEvent::failed(
            request,
            &format!("starting Google's sign-in: {err}"),
        ));
    }
}

fn start(request: u64, server_client_id: &str) -> jni::errors::Result<()> {
    balaur_platform::with_activity(|env, activity| {
        // Safety: `answer` takes the JNI environment, the class (a static
        // method), a long and a string, as `(JLjava/lang/String;)V` says.
        let shim = unsafe {
            balaur_platform::shim(
                env,
                activity,
                "org.balaur.google.SignIn",
                &[NativeMethod::from_raw_parts(
                    jni_str!("answer"),
                    jni_str!("(JLjava/lang/String;)V"),
                    answer as *mut c_void,
                )],
                &SHIM,
            )?
        };
        let client = env.new_string(server_client_id)?;
        env.call_static_method(
            shim,
            jni_str!("start"),
            jni_sig!("(Landroid/app/Activity;JLjava/lang/String;)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
                JValue::Object(&client),
            ],
        )?;
        Ok(())
    })
}

/// `SignIn.answer(long, String)`: the answer for a request, as JSON.
extern "system" fn answer<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    request: jlong,
    json: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let json = json.try_to_string(env)?;
        let request = u64::try_from(request).unwrap_or_default();
        let event = serde_json::from_str(&json).map_or_else(
            |err| GoogleEvent::failed(request, &format!("the shim's answer: {err}")),
            |answer| GoogleEvent { request, answer },
        );
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
