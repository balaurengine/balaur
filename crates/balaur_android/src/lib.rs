//! The Android entry point.
//!
//! Android never calls `fn main`. The NativeActivity declared in the APK loads
//! `libmain.so` and invokes `android_main` on a thread of its own, handing over
//! the activity handle that opening a window needs — which is why this is a
//! cdylib and not a binary.
//!
//! It also means a game cannot be fused the way a desktop game is. Appending a
//! pack to the executable works on a desktop because the OS runs that file;
//! here `current_exe` is the app process, not this library, and the library
//! lives inside a zip. So the pack travels as an APK asset, and exporting for
//! Android means writing `game.bpak` into `assets/` and re-signing — see
//! docs/PLAN-mobile-export.md.
#![cfg(target_os = "android")]

use kiss3d::winit::platform::android::activity::AndroidApp;

/// Where an exported game puts its pack inside the APK.
const PACK_ASSET: &std::ffi::CStr = c"game.bpak";

#[no_mangle]
fn android_main(app: AndroidApp) {
    balaur::logbuf::capture(tracing::level_filters::LevelFilter::INFO);

    // Read the pack before handing the handle over: `init_android` takes the
    // `AndroidApp` by value, and the asset manager comes off it.
    let assets = app.asset_manager();
    kiss3d::window::init_android(app);

    let Some(mut asset) = assets.open(PACK_ASSET) else {
        tracing::error!(
            "no {PACK_ASSET:?} in the APK assets: this is the bare template, \
             not an exported game"
        );
        return;
    };
    let bytes = match asset.buffer() {
        Ok(bytes) => bytes.to_vec(),
        Err(err) => {
            tracing::error!("reading {PACK_ASSET:?} from the APK: {err}");
            return;
        }
    };
    if let Err(err) = balaur::boot_pack(&bytes) {
        tracing::error!("the game stopped: {err:#}");
    }
}
