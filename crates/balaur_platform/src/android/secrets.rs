//! `secrets.*` on Android: `org.balaur.platform.Secrets` in the template's
//! `classes.dex` (`java/`), which seals each value with an AES key the
//! Keystore holds and keeps it in the app's private preferences.

use std::sync::OnceLock;

use jni::objects::{JClass, JObject, JString, JValue};
use jni::refs::Global;
use jni::{Env, jni_sig, jni_str};

const CLASS: &str = "org.balaur.platform.Secrets";

/// The shim, loaded once; it has no natives.
static SHIM: OnceLock<Global<JClass<'static>>> = OnceLock::new();

fn load<'cache>(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
) -> jni::errors::Result<&'cache Global<JClass<'static>>> {
    // Safety: no natives are registered.
    unsafe { super::shim(env, activity, CLASS, &[], &SHIM) }
}

/// Keep `value` under `key`: false where the Keystore or the preferences refused.
pub(crate) fn set(key: &str, value: &str) -> jni::errors::Result<bool> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let key = env.new_string(key)?;
        let value = env.new_string(value)?;
        env.call_static_method(
            shim,
            jni_str!("set"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Z"),
            &[
                JValue::Object(activity),
                JValue::Object(&key),
                JValue::Object(&value),
            ],
        )?
        .z()
    })
}

/// What `key` holds, or none.
pub(crate) fn get(key: &str) -> jni::errors::Result<Option<String>> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let key = env.new_string(key)?;
        let held = env
            .call_static_method(
                shim,
                jni_str!("get"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(activity), JValue::Object(&key)],
            )?
            .l()?;
        if held.is_null() {
            return Ok(None);
        }
        let held = env.cast_local::<JString<'_>>(held)?;
        Ok(Some(held.try_to_string(env)?))
    })
}

/// Forget what `key` holds: false where it held nothing.
pub(crate) fn remove(key: &str) -> jni::errors::Result<bool> {
    super::with_activity(|env, activity| {
        let shim = load(env, activity)?;
        let key = env.new_string(key)?;
        env.call_static_method(
            shim,
            jni_str!("remove"),
            jni_sig!("(Landroid/content/Context;Ljava/lang/String;)Z"),
            &[JValue::Object(activity), JValue::Object(&key)],
        )?
        .z()
    })
}
