//! The machine's time zone: its IANA name, `Europe/Madrid`, what a server
//! that keeps a player's zone by name is sent, and how far it is from UTC at
//! an instant. Each platform keeps it its own way, and each is asked on every
//! call: a zone changed while the game runs is what the next call answers.

/// The name of the zone the system is set to, or `None` where it names none.
#[must_use]
pub fn name() -> Option<String> {
    zone::name().filter(|name| !name.is_empty())
}

/// How far the zone is from UTC at `at` (seconds since 1970), in seconds
/// east, daylight saving included; 0 where the platform says nothing.
#[must_use]
pub fn offset(at: f64) -> i64 {
    utc::offset(at)
}

#[cfg(all(unix, not(target_family = "wasm")))]
mod utc {
    pub(super) fn offset(at: f64) -> i64 {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "whole seconds, in time_t's range"
        )]
        let instant = at.floor() as libc::time_t;
        // SAFETY: an all-zero `tm` is a valid value of the plain C struct,
        // and `localtime_r` writes into it alone, both live for the call.
        let mut broken: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers are to live locals of the right types.
        let out = unsafe { libc::localtime_r(&raw const instant, &raw mut broken) };
        if out.is_null() {
            return 0;
        }
        #[allow(
            clippy::useless_conversion,
            reason = "a c_long is 32 bits on some phones"
        )]
        i64::from(broken.tm_gmtoff)
    }
}

#[cfg(windows)]
mod utc {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{
        FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    };

    /// Seconds from 1601, where a FILETIME counts from, to 1970.
    const EPOCH_GAP: f64 = 11_644_473_600.0;
    /// A FILETIME's ticks in a second.
    const TICKS: i64 = 10_000_000;

    pub(super) fn offset(at: f64) -> i64 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a time after 1601, in 100 ns ticks"
        )]
        let ticks = ((at.floor() + EPOCH_GAP) * 1e7) as u64;
        #[allow(clippy::cast_possible_truncation, reason = "the FILETIME's two halves")]
        let utc_file = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        // SAFETY: plain C structs, all-zero valid; each call writes one of
        // them through a pointer to a live local, and none outlives it.
        let (mut utc, mut local, mut local_file): (SYSTEMTIME, SYSTEMTIME, FILETIME) =
            unsafe { (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed()) };
        // SAFETY: as above; a null zone is the machine's own.
        let done = unsafe {
            FileTimeToSystemTime(&raw const utc_file, &raw mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &raw const utc, &raw mut local)
                    != 0
                && SystemTimeToFileTime(&raw const local, &raw mut local_file) != 0
        };
        if !done {
            return 0;
        }
        let local_ticks =
            (u64::from(local_file.dwHighDateTime) << 32) | u64::from(local_file.dwLowDateTime);
        #[allow(clippy::cast_possible_wrap, reason = "ticks of a time in this era")]
        let gap = local_ticks as i64 - ticks as i64;
        gap / TICKS
    }
}

#[cfg(target_family = "wasm")]
mod utc {
    pub(super) fn offset(at: f64) -> i64 {
        let date = js_sys::Date::new(&js_sys::wasm_bindgen::JsValue::from_f64(at * 1000.0));
        // Minutes west, as JavaScript counts them.
        #[allow(clippy::cast_possible_truncation, reason = "whole minutes")]
        let west = date.get_timezone_offset() as i64;
        -west * 60
    }
}

#[cfg(not(any(unix, windows, target_family = "wasm")))]
mod utc {
    pub(super) fn offset(_at: f64) -> i64 {
        0
    }
}

/// CoreFoundation's system zone, read again rather than the copy it caches:
/// it honours `TZ`, and a change in System Settings reaches it.
#[cfg(target_vendor = "apple")]
mod zone {
    use std::ffi::{c_char, c_void};

    const UTF8: u32 = 0x0800_0100;
    /// Longer than any zone's name, which the database keeps under 30 bytes.
    const LONGEST: usize = 128;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFTimeZoneResetSystem();
        fn CFTimeZoneCopySystem() -> *const c_void;
        fn CFTimeZoneGetName(zone: *const c_void) -> *const c_void;
        fn CFStringGetCString(
            text: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRelease(object: *const c_void);
    }

    pub(super) fn name() -> Option<String> {
        let mut buffer = [0 as c_char; LONGEST];
        // SAFETY: the zone is copied (ours to release) and its name is got
        // (the zone's own, read before the release); the buffer is a live
        // local of the size passed.
        let written = unsafe {
            CFTimeZoneResetSystem();
            let zone = CFTimeZoneCopySystem();
            if zone.is_null() {
                return None;
            }
            let name = CFTimeZoneGetName(zone);
            #[allow(clippy::cast_possible_wrap, reason = "a small buffer")]
            let written = !name.is_null()
                && CFStringGetCString(name, buffer.as_mut_ptr(), LONGEST as isize, UTF8) != 0;
            CFRelease(zone);
            written
        };
        if !written {
            return None;
        }
        // SAFETY: CFStringGetCString wrote a terminated string into it.
        let text = unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) };
        text.to_str().ok().map(str::to_string)
    }
}

/// The property Android's time zone setting writes and bionic's `localtime`
/// reads.
#[cfg(target_os = "android")]
mod zone {
    pub(super) fn name() -> Option<String> {
        #[allow(clippy::cast_sign_loss, reason = "a positive constant")]
        let mut value = [0 as libc::c_char; libc::PROP_VALUE_MAX as usize];
        // SAFETY: the name is a terminated literal and the buffer holds the
        // PROP_VALUE_MAX bytes the call may write.
        let length = unsafe {
            libc::__system_property_get(c"persist.sys.timezone".as_ptr(), value.as_mut_ptr())
        };
        if length <= 0 {
            return None;
        }
        // SAFETY: the call wrote a terminated string of `length` bytes.
        let text = unsafe { std::ffi::CStr::from_ptr(value.as_ptr()) };
        text.to_str().ok().map(str::to_string)
    }
}

/// The page's: `Intl.DateTimeFormat().resolvedOptions().timeZone`.
#[cfg(target_family = "wasm")]
mod zone {
    pub(super) fn name() -> Option<String> {
        let format =
            js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new());
        js_sys::Reflect::get(&format.resolved_options(), &"timeZone".into())
            .ok()?
            .as_string()
    }
}

/// Windows names its zones its own way, `Romance Standard Time`: the name
/// is CLDR's IANA zone for it.
#[cfg(windows)]
mod zone {
    use windows_sys::Win32::System::Time::{
        DYNAMIC_TIME_ZONE_INFORMATION, GetDynamicTimeZoneInformation, TIME_ZONE_ID_INVALID,
    };

    pub(super) fn name() -> Option<String> {
        let mut info = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // SAFETY: a pointer to a live local the call fills.
        if unsafe { GetDynamicTimeZoneInformation(&raw mut info) } == TIME_ZONE_ID_INVALID {
            return None;
        }
        let key = &info.TimeZoneKeyName;
        let end = key.iter().position(|&unit| unit == 0).unwrap_or(key.len());
        let key = String::from_utf16_lossy(&key[..end]);
        super::windows_zones::iana(&key).map(str::to_string)
    }
}

/// `TZ` where it names a zone, else the zone file `/etc/localtime` links
/// to, else Debian's `/etc/timezone`.
#[cfg(all(
    unix,
    not(any(target_vendor = "apple", target_os = "android", target_family = "wasm"))
))]
mod zone {
    use super::in_zoneinfo;

    /// The variable a Unix reads its zone from first.
    const TZ: &str = "TZ";
    const LOCALTIME: &str = "/etc/localtime";
    const TIMEZONE: &str = "/etc/timezone";
    const ZONEINFO: &str = "/usr/share/zoneinfo";

    pub(super) fn name() -> Option<String> {
        if let Some(name) = std::env::var(TZ).ok().and_then(|tz| from_tz(&tz)) {
            return Some(name);
        }
        if let Some(name) = linked_zone(LOCALTIME) {
            return Some(name);
        }
        let named = std::fs::read_to_string(TIMEZONE); // os files: the system's zone
        named.ok().map(|text| text.trim().to_string())
    }

    /// The zone the file at `path` is in the database, or the one it links to:
    /// `/etc/localtime` is a link to one.
    fn linked_zone(path: &str) -> Option<String> {
        let to = std::fs::read_link(path).ok()?; // os files: the system's zone
        in_zoneinfo(&to.to_string_lossy())
    }

    /// The zone `TZ` names: a file under the database, by path or by name.
    /// An empty one is UTC; a rule spelled out (`CET-1CEST,M3.5.0,…`) or a
    /// file outside the database names none, and the system's is read.
    fn from_tz(tz: &str) -> Option<String> {
        let tz = tz.strip_prefix(':').unwrap_or(tz);
        if tz.is_empty() {
            return Some("UTC".to_string());
        }
        if tz.starts_with('/') {
            return in_zoneinfo(tz).or_else(|| linked_zone(tz));
        }
        let file = std::path::Path::new(ZONEINFO).join(tz);
        file.is_file().then(|| tz.to_string()) // os files: the zone database
    }
}

#[cfg(not(any(unix, windows, target_family = "wasm")))]
mod zone {
    pub(super) const fn name() -> Option<String> {
        None
    }
}

/// The zone a path into the zone database names:
/// `/usr/share/zoneinfo/Europe/Madrid` is `Europe/Madrid`, and so is the
/// database's `posix/` copy of it.
#[cfg(any(
    test,
    all(unix, not(any(target_vendor = "apple", target_os = "android")))
))]
fn in_zoneinfo(path: &str) -> Option<String> {
    const DATABASE: &str = "zoneinfo/";
    let at = path.rfind(DATABASE)? + DATABASE.len();
    let name = &path[at..];
    let name = ["posix/", "right/"]
        .iter()
        .find_map(|copy| name.strip_prefix(copy))
        .unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(any(test, windows))]
mod windows_zones;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_into_the_zone_database_names_its_zone() {
        for (path, zone) in [
            ("/usr/share/zoneinfo/Europe/Madrid", Some("Europe/Madrid")),
            (
                "../usr/share/zoneinfo/America/New_York",
                Some("America/New_York"),
            ),
            ("/var/db/timezone/zoneinfo/Asia/Tokyo", Some("Asia/Tokyo")),
            (
                "/usr/share/zoneinfo/posix/Europe/Paris",
                Some("Europe/Paris"),
            ),
            ("/etc/zoneinfo/UTC", Some("UTC")),
            ("/usr/share/zoneinfo/", None),
            ("/etc/localtime", None),
        ] {
            assert_eq!(in_zoneinfo(path).as_deref(), zone, "{path}");
        }
    }

    #[test]
    fn a_windows_zone_is_named_as_cldr_names_it() {
        for (windows, zone) in [
            ("Romance Standard Time", Some("Europe/Paris")),
            ("W. Europe Standard Time", Some("Europe/Berlin")),
            ("GTB Standard Time", Some("Europe/Bucharest")),
            ("Pacific Standard Time", Some("America/Los_Angeles")),
            ("UTC", Some("Etc/UTC")),
            ("UTC+12", Some("Etc/GMT-12")),
            ("Tokyo Standard Time", Some("Asia/Tokyo")),
            ("Atlantis Standard Time", None),
        ] {
            assert_eq!(windows_zones::iana(windows), zone, "{windows}");
        }
    }

    #[test]
    fn every_windows_zone_is_named_once() {
        let mut seen = std::collections::HashSet::new();
        for (windows, zone) in windows_zones::ZONES {
            assert!(seen.insert(windows), "{windows} is named twice");
            assert!(!zone.is_empty() && !zone.contains(' '), "{windows}: {zone}");
        }
    }

    #[test]
    fn this_machine_names_a_zone_where_it_keeps_one() {
        // A container may keep no zone; a Mac and a desktop always do.
        if cfg!(target_vendor = "apple") {
            let zone = name().expect("a Mac names its zone");
            assert!(
                zone.contains('/') || zone == "UTC" || zone == "GMT",
                "{zone}"
            );
        }
    }
}
