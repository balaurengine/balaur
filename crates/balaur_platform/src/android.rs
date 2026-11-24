//! Android's JNI, shared by every crate with a Java shim in the template's
//! `classes.dex` (`crates/*/java`): the activity, and a shim loaded through it.
//!
//! The game's thread is a native one, and `FindClass` from it sees only the
//! platform's classes, so a shim is loaded through the activity's own class
//! loader. Its native methods are registered on it here rather than looked up
//! by a mangled symbol, since NativeActivity loads this library itself.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use jni::objects::{JClass, JObject, JValue};
use jni::refs::Global;
use jni::{Env, JavaVM, NativeMethod, jni_sig, jni_str};

pub(crate) mod web_sign_in;

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
