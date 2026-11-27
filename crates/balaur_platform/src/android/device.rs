//! The motor, the share sheet, the photo picker and which services work here:
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
pub(crate) const SERVICES: [Service; 5] = [
    Service::Notifications,
    Service::Haptics,
    Service::Share,
    Service::PhotoPicker,
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

/// The system's photo picker over the game; `PickPhoto` answers once it shuts.
pub(crate) fn pick_photo(request: u64) -> jni::errors::Result<()> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        env.call_static_method(
            shim,
            jni_str!("pickPhoto"),
            jni_sig!("(Landroid/app/Activity;J)V"),
            &[
                JValue::Object(activity),
                JValue::Long(jlong::try_from(request).unwrap_or(jlong::MAX)),
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

/// Whether Android's Remove animations is on: the animator duration scale
/// at 0, which is what the switch sets. `None` when the setting cannot be read.
fn reduced_motion() -> Option<bool> {
    super::with_activity(|env, activity| {
        let resolver = env
            .call_method(
                activity,
                jni_str!("getContentResolver"),
                jni_sig!("()Landroid/content/ContentResolver;"),
                &[],
            )?
            .l()?;
        let settings = env.find_class(jni_str!("android/provider/Settings$Global"))?;
        let key = env.new_string("animator_duration_scale")?;
        let scale = env
            .call_static_method(
                &settings,
                jni_str!("getFloat"),
                jni_sig!("(Landroid/content/ContentResolver;Ljava/lang/String;F)F"),
                &[
                    JValue::Object(&resolver),
                    JValue::Object(&key),
                    JValue::Float(1.0),
                ],
            )?
            .f()?;
        Ok(scale.abs() < f32::EPSILON)
    })
    .ok()
}

thread_local! {
    /// Whether the game was in front and awake when the setting was last read.
    static READ_AT: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) };
}

/// Read Remove animations into the device facts on the first tick and each
/// time the game comes back to the front, which is where a player who
/// changed it in Settings returns from.
pub(crate) fn follow_reduced_motion_system(eng: &balaur_core::Engine, _: f32) {
    let facts = balaur_core::facts::device(eng);
    let now = Some((facts.focused, facts.suspended));
    if READ_AT.get() == now {
        return;
    }
    READ_AT.set(now);
    if let Some(reduced) = reduced_motion() {
        balaur_core::facts::update_device(eng, |facts| facts.reduced_motion = reduced);
    }
}
