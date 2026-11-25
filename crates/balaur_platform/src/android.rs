//! Android's JNI, shared by every crate with a Java shim in the template's
//! `classes.dex` (`crates/*/java`): the activity, and a shim loaded through it.
//!
//! The game's thread is a native one, and `FindClass` from it sees only the
//! platform's classes, so a shim is loaded through the activity's own class
//! loader. Its native methods are registered on it here rather than looked up
//! by a mangled symbol, since NativeActivity loads this library itself.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use std::sync::mpsc::Sender;
use std::sync::{Mutex, PoisonError};

use jni::objects::{JClass, JObject, JString, JValue};
use jni::refs::Global;
use jni::strings::JNIString;
use jni::sys::jlong;
use jni::{Env, EnvUnowned, JavaVM, NativeMethod, jni_sig, jni_str};

use crate::PlatformEvent;
use crate::device::DeviceCall;

pub(crate) mod device;
mod links;
pub(crate) mod notifications;
pub(crate) mod web_sign_in;

/// Where the device shims' answers go: the platform's channel, which every
/// call shares.
static REPORT: Mutex<Option<Sender<PlatformEvent>>> = Mutex::new(None);

/// What a shim reported unasked (request 0), until a pump allowed to touch
/// the outside moves it, as Apple's queue does.
static ARRIVALS: Mutex<Vec<PlatformEvent>> = Mutex::new(Vec::new());

/// Play's review prompt, which a game's dex carries with `[android]
/// capabilities = ["in-app-review"]`.
pub(crate) const REVIEW: &str = "org.balaur.google.Review";

/// Firebase Cloud Messaging, which a game's dex carries with `[android]
/// capabilities = ["firebase-messaging"]`.
pub(crate) const PUSH: &str = "org.balaur.google.Push";

/// What `register_for_push` says when the export left Firebase out.
const PUSH_MISSING: &str =
    "push needs [android] capabilities = [\"firebase-messaging\"], which this export left out";

/// Whether the push shim was told to hand its arrivals over, or found absent.
static PUSH_LISTENING: AtomicBool = AtomicBool::new(false);

/// A call to the OS, through the shim that makes it.
pub(crate) fn start(request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>) {
    // copy: the channel outlives this call, until the answer crosses it.
    *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(report.clone());
    let started = match call {
        DeviceCall::RequestNotifications => notifications::request(request),
        DeviceCall::Notify {
            id,
            title,
            body,
            after,
            data,
        } => notifications::schedule(request, id, title, body, *after, data),
        DeviceCall::Share { text, file, title } => {
            device::share(request, text, file.as_deref(), title)
        }
        DeviceCall::RequestReview => {
            start_capability(REVIEW, request, call.name(), &serde_json::json!({}), report).map(
                |started| {
                    if !started {
                        crate::native::unsupported(request, call, report);
                    }
                },
            )
        }
        DeviceCall::RegisterForPush => {
            start_capability(PUSH, request, call.name(), &serde_json::json!({}), report).map(
                |started| {
                    if !started {
                        let _ = report.send(PlatformEvent::PushFailed {
                            request,
                            message: PUSH_MISSING.into(),
                        });
                    }
                },
            )
        }
    };
    if let Err(err) = started {
        let _ = report.send(PlatformEvent::Failed {
            request,
            message: format!("{}: {err}", call.name()),
        });
    }
}

/// The native method a device shim answers through, `answer(long, String)`:
/// a request's answer as the JSON map its waiter receives, or under request
/// 0 what arrived unasked.
pub(crate) fn answer_native() -> NativeMethod<'static> {
    // Safety: `answer` takes the JNI environment, the class (a static
    // method), a long and a string, as its signature says.
    unsafe {
        NativeMethod::from_raw_parts(
            jni_str!("answer"),
            jni_str!("(JLjava/lang/String;)V"),
            answer as *mut std::ffi::c_void,
        )
    }
}

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
            |err| PlatformEvent::Failed {
                request,
                message: format!("the shim's answer: {err}"),
            },
            |answer| PlatformEvent::Answer { request, answer },
        );
        if request == 0 {
            ARRIVALS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(event);
        } else if let Some(report) = REPORT
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

/// Tell the push shim to hand over what arrived (a token, a push, the tap
/// that launched the game), once the activity is there. A game whose dex
/// has no shim is asked once.
pub(crate) fn listen_for_push(report: &Sender<PlatformEvent>) {
    if PUSH_LISTENING.load(Ordering::Acquire) {
        return;
    }
    if start_capability(PUSH, 0, LISTEN, &serde_json::json!({}), report).is_ok() {
        PUSH_LISTENING.store(true, Ordering::Release);
    }
}

const LISTEN: &str = "listen";

/// Tell a shim to hand over what arrived (its static `listen(Activity)`),
/// once the activity is there; `listening` keeps that it was told.
fn listen_once(
    listening: &AtomicBool,
    load: impl FnOnce(
        &mut Env<'_>,
        &JObject<'_>,
    ) -> jni::errors::Result<&'static Global<JClass<'static>>>,
) {
    if listening.load(Ordering::Acquire) {
        return;
    }
    let listened = with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            JNIString::new(LISTEN),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(activity)],
        )?;
        Ok(())
    });
    if listened.is_ok() {
        listening.store(true, Ordering::Release);
    }
}

/// Listen on every shim once the activity is there (a tap, a link, a push),
/// then move what arrived onto the channel.
pub(crate) fn pump(report: &Sender<PlatformEvent>) {
    notifications::listen();
    links::listen();
    listen_for_push(report);
    let arrived = std::mem::take(&mut *ARRIVALS.lock().unwrap_or_else(PoisonError::into_inner));
    for event in arrived {
        balaur_core::replay::report(report, event);
    }
}

/// What arrived during a replay is not this run's to deliver.
pub(crate) fn discard() {
    ARRIVALS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

/// The Java `Activity` (`keep_activity`), which a sheet or a page is shown
/// over. `ndk-context` holds the `Application`, which cannot.
static ACTIVITY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Keep the activity's JNI reference, which its owner keeps valid for the
/// process. The facade hands it over as the app is built.
pub fn keep_activity(activity: *mut c_void) {
    ACTIVITY.store(activity, Ordering::Release);
}

/// Run `body` on this thread, attached to the VM, with the activity.
///
/// # Errors
/// When the activity was never kept, or a JNI call in `body` fails.
pub fn with_activity<T>(
    body: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> jni::errors::Result<T> {
    let context = ndk_context::android_context();
    let raw = ACTIVITY.load(Ordering::Acquire);
    if context.vm().is_null() || raw.is_null() {
        return Err(jni::errors::Error::NullPtr("the activity"));
    }
    // Safety: ndk-context's VM pointer is the process's, checked non-null.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) };
    vm.attach_current_thread(|env| {
        // Safety: android-activity's global reference to the activity,
        // checked non-null and kept for the process; this never deletes it.
        let activity = unsafe { JObject::from_raw(env, raw.cast()) };
        body(env, &activity)
    })
}

/// The shim class `name`, loaded through the activity's class loader and
/// `natives` registered on it the first time; `cache` keeps it.
///
/// # Errors
/// When the template's dex has no such class, or a native does not bind.
///
/// # Safety
/// Each of `natives` must point at a function taking what its signature says.
pub unsafe fn shim<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    name: &str,
    natives: &[NativeMethod<'_>],
    cache: &'cache OnceLock<Global<JClass<'static>>>,
) -> jni::errors::Result<&'cache Global<JClass<'static>>> {
    if let Some(shim) = cache.get() {
        return Ok(shim);
    }
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let name = env.new_string(name)?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    let class = env.cast_local::<JClass<'_>>(class)?;
    // Safety: the caller vouches for each native's signature.
    unsafe { env.register_native_methods(&class, natives)? };
    let global = env.new_global_ref(class)?;
    Ok(cache.get_or_init(|| global))
}

/// The capability shims loaded so far, by class name. An export keeps a
/// capability's shim only when `[android] capabilities` names it.
static CAPABILITY_SHIMS: Mutex<Vec<(String, Global<JClass<'static>>)>> = Mutex::new(Vec::new());

/// The capability shim `class`, loaded with the platform's `answer`
/// registered on it, or `None` when this game's dex does not carry it.
fn capability<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    class: &str,
    held: &'cache mut Vec<(String, Global<JClass<'static>>)>,
) -> jni::errors::Result<Option<&'cache Global<JClass<'static>>>> {
    if let Some(at) = held.iter().position(|(name, _)| name == class) {
        return Ok(Some(&held[at].1));
    }
    let cache = OnceLock::new();
    // Safety: `answer_native` points at a function of its own signature.
    let loaded = unsafe { shim(env, activity, class, &[answer_native()], &cache) };
    match loaded {
        Ok(_) => {}
        Err(jni::errors::Error::JavaException) => {
            // ClassNotFoundException: the export left this capability out.
            env.exception_clear();
            return Ok(None);
        }
        Err(err) => return Err(err),
    }
    let Some(global) = cache.into_inner() else {
        return Ok(None);
    };
    held.push((class.to_string(), global));
    Ok(held.last().map(|(_, global)| global))
}

/// Start `call` on the capability shim `class` (its static
/// `start(Activity, long, String, String)`), answering through the platform's
/// channel. False when the game's dex does not carry the shim.
///
/// # Errors
/// When the activity is gone or a JNI call fails.
pub fn start_capability(
    class: &str,
    request: u64,
    call: &str,
    args: &serde_json::Value,
    report: &Sender<PlatformEvent>,
) -> jni::errors::Result<bool> {
    // copy: the channel outlives this call, until the answer crosses it.
    *REPORT.lock().unwrap_or_else(PoisonError::into_inner) = Some(report.clone());
    with_activity(|env, activity| {
        let mut held = CAPABILITY_SHIMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(shim) = capability(env, activity, class, &mut held)? else {
            return Ok(false);
        };
        let call = env.new_string(call)?;
        let args = env.new_string(args.to_string())?;
        let started = env.call_static_method(
            shim,
            jni_str!("start"),
            jni_sig!("(Landroid/app/Activity;JLjava/lang/String;Ljava/lang/String;)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
                JValue::Object(&call),
                JValue::Object(&args),
            ],
        );
        if started.is_err() {
            env.exception_clear();
        }
        started.map(|_| true)
    })
}

/// Whether the capability shim `class` is in this game's dex and says its
/// service is here (its static `available(Activity)`).
pub fn capability_available(class: &str) -> bool {
    with_activity(|env, activity| {
        let mut held = CAPABILITY_SHIMS
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(shim) = capability(env, activity, class, &mut held)? else {
            return Ok(false);
        };
        let answered = env
            .call_static_method(
                shim,
                jni_str!("available"),
                jni_sig!("(Landroid/app/Activity;)Z"),
                &[JValue::Object(activity)],
            )
            .and_then(jni::JValueOwned::z);
        if answered.is_err() {
            env.exception_clear();
        }
        answered
    })
    .unwrap_or(false)
}
