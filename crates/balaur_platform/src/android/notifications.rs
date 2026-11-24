//! Local notifications: `org.balaur.platform.Notifications` in the template's
//! `classes.dex` (`java/`), with `AlarmManager` holding the schedule and a
//! channel of the game's own.
//!
//! A tap lands on `org.balaur.platform.Opened`, which brings the game forward
//! and hands the shim what the notification carried. The shim holds it until
//! the game listens, the first tick this runs, so a tap that started the
//! process arrives too, as an answer to request 0.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use jni::objects::{JClass, JObject, JValue};
use jni::refs::Global;
use jni::sys::jlong;
use jni::{Env, jni_sig, jni_str};

use crate::PlatformEvent;

const CLASS: &str = "org.balaur.platform.Notifications";

/// The shim, loaded and its native methods registered once.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

/// Whether the shim was told to hand its arrivals over.
static LISTENING: AtomicBool = AtomicBool::new(false);

/// Ask for `POST_NOTIFICATIONS` (Android 13 and later), or say whether the
/// player left notifications on.
pub(crate) fn request(request: u64) -> jni::errors::Result<()> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            jni_str!("request"),
            jni_sig!("(Landroid/app/Activity;J)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
            ],
        )?;
        Ok(())
    })
}

/// Put one notification on the schedule, `after` seconds from now.
pub(crate) fn schedule(
    request: u64,
    id: &str,
    title: &str,
    body: &str,
    after: f64,
    data: &serde_json::Value,
) -> jni::errors::Result<()> {
    #[allow(clippy::cast_possible_truncation, reason = "milliseconds from now")]
    let delay = (after * 1000.0).round().max(0.0) as jlong;
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let id = env.new_string(id)?;
        let title = env.new_string(title)?;
        let body = env.new_string(body)?;
        let data = env.new_string(data.to_string())?;
        env.call_static_method(
            shim,
            jni_str!("schedule"),
            jni_sig!(
                "(Landroid/app/Activity;JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;JLjava/lang/String;)V"
            ),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
                JValue::Object(&id),
                JValue::Object(&title),
                JValue::Object(&body),
                JValue::Long(delay),
                JValue::Object(&data),
            ],
        )?;
        Ok(())
    })
}

/// Take a notification off the schedule and out of the shade.
pub(crate) fn cancel(id: &str) {
    let cancelled = super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let id = env.new_string(id)?;
        env.call_static_method(
            shim,
            jni_str!("cancel"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
            &[JValue::Object(activity), JValue::Object(&id)],
        )?;
        Ok(())
    });
    if let Err(err) = cancelled {
        tracing::warn!("platform: cancelling a notification: {err}");
    }
}

/// Listen once the activity is there, then move what arrived onto the channel.
pub(crate) fn pump(report: &Sender<PlatformEvent>) {
    if !LISTENING.load(Ordering::Acquire) && listen().is_ok() {
        LISTENING.store(true, Ordering::Release);
    }
    super::pump(report);
}

pub(crate) use super::discard;

fn listen() -> jni::errors::Result<()> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            jni_str!("listen"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(activity)],
        )?;
        Ok(())
    })
}

fn load<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<&'cache Global<JClass<'static>>> {
    // Safety: `answer_native` points at a function of its own signature.
    unsafe { super::shim(env, activity, CLASS, &[super::answer_native()], &SHIM) }
}
