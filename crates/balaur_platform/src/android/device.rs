//! The motor, the share sheet and which services work here:
//! `org.balaur.platform.Device` in the template's `classes.dex` (`java/`).
//!
//! The share sheet answers on the UI thread once it shuts, through `answer`,
//! as the JSON map its waiter receives.

use std::path::Path;
use std::sync::OnceLock;

use jni::objects::{JClass, JObject, JValue};
use jni::refs::Global;
use jni::sys::jlong;
use jni::{Env, jni_sig, jni_str};

use crate::device::Haptic;
use crate::services::Service;

const CLASS: &str = "org.balaur.platform.Device";

/// What this shim answers `is_available` for.
pub(crate) const SERVICES: [Service; 4] = [
    Service::Notifications,
    Service::Haptics,
    Service::Share,
    Service::WebSignIn,
];

/// The shim, loaded and its native method registered once.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

/// Whether `service` works on this device; false when the shim cannot say.
pub(crate) fn available(service: Service) -> bool {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let name = env.new_string(service.name())?;
        env.call_static_method(
            shim,
            jni_str!("available"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)Z"),
            &[JValue::Object(activity), JValue::Object(&name)],
        )?
        .z()
    })
    .unwrap_or(false)
}

/// Buzz the vibrator for `milliseconds`.
pub(crate) fn vibrate(milliseconds: u32) {
    let buzzed = super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            jni_str!("vibrate"),
            jni_sig!("(Landroid/app/Activity;J)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::from(milliseconds)),
            ],
        )?;
        Ok(())
    });
    if let Err(err) = buzzed {
        tracing::debug!("device: vibrating: {err}");
    }
}

/// One of the vibrator's predefined effects, or a short buzz below Android 10.
pub(crate) fn haptic(effect: Haptic) {
    let tapped = super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let name = env.new_string(effect.name())?;
        env.call_static_method(
            shim,
            jni_str!("haptic"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
            &[JValue::Object(activity), JValue::Object(&name)],
        )?;
        Ok(())
    });
    if let Err(err) = tapped {
        tracing::debug!("device: haptic: {err}");
    }
}

/// The system's chooser over the game, with `text`, `file` or both.
pub(crate) fn share(
    request: u64,
    text: &str,
    file: Option<&Path>,
    title: &str,
) -> jni::errors::Result<()> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let text = env.new_string(text)?;
        let file = env.new_string(file.map(|path| path.to_string_lossy()).unwrap_or_default())?;
        let title = env.new_string(title)?;
        env.call_static_method(
            shim,
            jni_str!("share"),
            jni_sig!(
                "(Landroid/app/Activity;JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"
            ),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
                JValue::Object(&text),
                JValue::Object(&file),
                JValue::Object(&title),
            ],
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
