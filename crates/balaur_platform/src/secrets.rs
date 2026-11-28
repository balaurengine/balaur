//! `secrets.*`: a small secret kept by the OS, a session's refresh token, by
//! key. Apple's Keychain, an Android Keystore key sealing each value, the
//! Windows Credential Manager, the Secret Service on Linux through
//! `secret-tool`, a page's `localStorage`. Where none answers, and in a test
//! whose stand-ins name `"secrets.backend": "file"`, a file under the user
//! directory that only this user may read.
//!
//! Read and written in the call, never recorded: a replay reads what the
//! machine holds now, as `utc_offset` does.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use balaur_core::Engine;
use balaur_script::{Module, Value};

/// The script module.
pub const SECRETS: &str = "secrets";

const BACKEND: &str = "backend";
const GET: &str = "get";
const REMOVE: &str = "remove";
const SET: &str = "set";

/// Where the secrets are kept, as `secrets::backend()` names it.
pub mod backends {
    pub const KEYCHAIN: &str = "keychain";
    pub const KEYSTORE: &str = "keystore";
    pub const CREDENTIAL_MANAGER: &str = "credential_manager";
    pub const SECRET_SERVICE: &str = "secret_service";
    pub const LOCAL_STORAGE: &str = "local_storage";
    pub const FILE: &str = "file";
    pub const ALL: [&str; 6] = [
        KEYCHAIN,
        KEYSTORE,
        CREDENTIAL_MANAGER,
        SECRET_SERVICE,
        LOCAL_STORAGE,
        FILE,
    ];
}

/// The file the file store keeps, under the user directory.
const FILE_NAME: &str = "secrets.json";

/// What every store keeps a secret under: the game, then the key.
struct Place {
    service: String,
    key: String,
}

impl Place {
    /// One name for both, where a store keys by a single string.
    #[cfg_attr(
        not(any(target_os = "android", target_family = "wasm")),
        allow(dead_code, reason = "Android's and a page's stores key by one string")
    )]
    fn item(&self) -> String {
        format!("{}.{}", self.service, self.key)
    }
}

/// Where this run keeps its secrets: the file a stand-in names, or the OS's
/// own store, which on a Linux with no `secret-tool` is that file too.
enum Store {
    File(PathBuf),
    Os,
}

impl Store {
    fn of(eng: &Engine) -> Self {
        let named = matches!(
            crate::stand_in(eng, SECRETS, BACKEND),
            Some(Some(serde_json::Value::String(named))) if named == backends::FILE
        );
        if named || os::name() == backends::FILE {
            Self::File(balaur_core::engine_api::data_home(eng).join(FILE_NAME))
        } else {
            Self::Os
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::File(_) => backends::FILE,
            Self::Os => os::name(),
        }
    }

    fn set(&self, place: &Place, value: &str) -> Result<()> {
        match self {
            Self::File(path) => file::set(path, place, value),
            Self::Os => os::set(place, value),
        }
    }

    fn get(&self, place: &Place) -> Result<Option<String>> {
        match self {
            Self::File(path) => file::get(path, place),
            Self::Os => os::get(place),
        }
    }

    fn remove(&self, place: &Place) -> Result<bool> {
        match self {
            Self::File(path) => file::remove(path, place),
            Self::Os => os::remove(place),
        }
    }
}

fn place(eng: &Engine, key: &str) -> Result<Place> {
    anyhow::ensure!(!key.is_empty(), "a secret's key is not empty");
    let service = eng
        .try_resource::<balaur_core::project::ProjectManifest>()
        .map_or_else(
            || "balaur.".to_string(),
            |m| format!("balaur.{}", m.borrow().name),
        );
    Ok(Place {
        service,
        key: key.to_string(),
    })
}

/// `secrets.*`.
pub fn install(m: &mut Module<Engine>) {
    m.module_doc(
        "A small secret kept by the OS, by key: a session's refresh token. Apple's Keychain, an Android Keystore key sealing each value, the Windows Credential Manager, the Secret Service on Linux (`secret-tool`), else a file under the user directory only this user may read. A page keeps it in `localStorage`, which any script on the page can read: not secure. Read and written in the call, and never recorded. A test keeps them in that file with `\"secrets.backend\": \"file\"` in the file `BALAUR_STAND_INS` names.",
    );
    for word in backends::ALL {
        m.constant(
            &format!("BACKEND_{}", word.to_ascii_uppercase()),
            Value::Str(word.into()),
        );
    }
    m.describe(&[
        (
            SET,
            &[],
            "(key: string, value: string) -> bool",
            "Keep `value` under `key`, over what it held. False where the store refused, with the reason logged.",
        ),
        (
            GET,
            &[],
            "(key: string) -> string?",
            "What `key` holds, or nil where it holds nothing or the store cannot be read.",
        ),
        (
            REMOVE,
            &[],
            "(key: string) -> bool",
            "Forget what `key` holds. False where it held nothing.",
        ),
        (
            BACKEND,
            &[],
            "() -> string",
            "Where this run keeps its secrets, a `BACKEND_*` constant.",
        ),
    ]);
    m.function(BACKEND, |eng: &Engine, ()| {
        Ok(Value::Str(Store::of(eng).name().into()))
    });
    m.function(SET, |eng: &Engine, (key, value): (String, String)| {
        let place = place(eng, &key)?;
        let kept = Store::of(eng).set(&place, &value);
        Ok(Value::Bool(match kept {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!("secrets: keeping {key}: {err:#}");
                false
            }
        }))
    });
    m.function(GET, |eng: &Engine, key: String| {
        let place = place(eng, &key)?;
        let held = Store::of(eng).get(&place);
        Ok(match held {
            Ok(held) => held.map_or(Value::Nil, Value::Str),
            Err(err) => {
                tracing::warn!("secrets: reading {key}: {err:#}");
                Value::Nil
            }
        })
    });
    m.function(REMOVE, |eng: &Engine, key: String| {
        let place = place(eng, &key)?;
        let removed = Store::of(eng).remove(&place);
        Ok(Value::Bool(removed.unwrap_or_else(|err| {
            tracing::warn!("secrets: removing {key}: {err:#}");
            false
        })))
    });
}

/// A JSON object of key to value, written whole, readable by this user alone.
mod file {
    use super::{BTreeMap, Place, Result};
    use std::path::Path;

    fn read(path: &Path) -> Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(err.into()),
        }
    }

    /// Written beside the file and moved over it, so a write cut short never
    /// leaves a file the next read refuses.
    fn write(path: &Path, held: &BTreeMap<String, String>) -> Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial = path.with_extension("part");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        {
            // Closed before it takes the place of what was there.
            let mut file = options.open(&partial)?;
            file.write_all(serde_json::to_string_pretty(held)?.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&partial, path)?;
        Ok(())
    }

    pub(super) fn set(path: &Path, place: &Place, value: &str) -> Result<()> {
        let mut held = read(path)?;
        held.insert(place.key.clone(), value.to_string());
        write(path, &held)
    }

    pub(super) fn get(path: &Path, place: &Place) -> Result<Option<String>> {
        Ok(read(path)?.remove(&place.key))
    }

    pub(super) fn remove(path: &Path, place: &Place) -> Result<bool> {
        let mut held = read(path)?;
        if held.remove(&place.key).is_none() {
            return Ok(false);
        }
        write(path, &held)?;
        Ok(true)
    }
}

/// Apple's Keychain: one generic password per key, under the game's service.
#[cfg(target_vendor = "apple")]
mod os {
    use super::{Place, Result, backends};
    use anyhow::anyhow;
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;

    #[repr(C)]
    struct Opaque {
        _private: [u8; 0],
    }

    const UTF8: u32 = 0x0800_0100;
    const SUCCESS: i32 = 0;
    const NOT_FOUND: i32 = -25300;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFTypeDictionaryKeyCallBacks: Opaque;
        static kCFTypeDictionaryValueCallBacks: Opaque;
        static kCFBooleanTrue: CFTypeRef;
        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            key_callbacks: *const Opaque,
            value_callbacks: *const Opaque,
        ) -> CFTypeRef;
        fn CFStringCreateWithBytes(
            allocator: CFTypeRef,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> CFTypeRef;
        fn CFDataCreate(allocator: CFTypeRef, bytes: *const u8, length: isize) -> CFTypeRef;
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFDataGetLength(data: CFTypeRef) -> isize;
        fn CFRelease(object: CFTypeRef);
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecClass: CFTypeRef;
        static kSecClassGenericPassword: CFTypeRef;
        static kSecAttrService: CFTypeRef;
        static kSecAttrAccount: CFTypeRef;
        static kSecAttrAccessible: CFTypeRef;
        static kSecAttrAccessibleAfterFirstUnlock: CFTypeRef;
        static kSecValueData: CFTypeRef;
        static kSecReturnData: CFTypeRef;
        static kSecMatchLimit: CFTypeRef;
        static kSecMatchLimitOne: CFTypeRef;
        fn SecItemAdd(attributes: CFTypeRef, result: *mut CFTypeRef) -> i32;
        fn SecItemCopyMatching(query: CFTypeRef, result: *mut CFTypeRef) -> i32;
        fn SecItemDelete(query: CFTypeRef) -> i32;
    }

    pub(super) const fn name() -> &'static str {
        backends::KEYCHAIN
    }

    /// A Core Foundation object this side created, released when dropped.
    struct Owned(CFTypeRef);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // Safety: created with a Create function, so this side owns one count.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn string(text: &str) -> Owned {
        // Safety: the bytes are valid for `len` and copied by the call.
        Owned(unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                isize::try_from(text.len()).unwrap_or(isize::MAX),
                UTF8,
                0,
            )
        })
    }

    /// The query naming one item, with `extra` pairs after it.
    fn query(place: &Place, extra: &[(CFTypeRef, CFTypeRef)]) -> Owned {
        let service = string(&place.service);
        let account = string(&place.key);
        // Safety: the Security constants are immutable CFStrings the framework exports.
        let mut pairs = unsafe {
            vec![
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, service.0),
                (kSecAttrAccount, account.0),
            ]
        };
        pairs.extend_from_slice(extra);
        let keys: Vec<CFTypeRef> = pairs.iter().map(|(key, _)| *key).collect();
        let values: Vec<CFTypeRef> = pairs.iter().map(|(_, value)| *value).collect();
        // Safety: both arrays hold `count` CF objects, which the dictionary
        // retains through the CFType callbacks.
        Owned(unsafe {
            CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                isize::try_from(pairs.len()).unwrap_or(0),
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            )
        })
    }

    fn refused(what: &str, status: i32) -> anyhow::Error {
        anyhow!("the keychain refused to {what} (OSStatus {status})")
    }

    pub(super) fn set(place: &Place, value: &str) -> Result<()> {
        // Safety: a query built above, read by the call.
        let removed = unsafe { SecItemDelete(query(place, &[]).0) };
        if removed != SUCCESS && removed != NOT_FOUND {
            return Err(refused("replace a secret", removed));
        }
        // Safety: the bytes are valid for their length and copied by the call.
        let data = Owned(unsafe {
            CFDataCreate(
                std::ptr::null(),
                value.as_ptr(),
                isize::try_from(value.len()).unwrap_or(isize::MAX),
            )
        });
        // After first unlock: a refresh that runs while the phone is locked
        // still reads it.
        // Safety: immutable framework constants.
        let attributes = unsafe {
            query(
                place,
                &[
                    (kSecValueData, data.0),
                    (kSecAttrAccessible, kSecAttrAccessibleAfterFirstUnlock),
                ],
            )
        };
        // Safety: the attributes dictionary built above; no result asked for.
        let added = unsafe { SecItemAdd(attributes.0, std::ptr::null_mut()) };
        if added == SUCCESS {
            Ok(())
        } else {
            Err(refused("keep a secret", added))
        }
    }

    pub(super) fn get(place: &Place) -> Result<Option<String>> {
        // Safety: immutable framework constants.
        let asked = unsafe {
            query(
                place,
                &[
                    (kSecReturnData, kCFBooleanTrue),
                    (kSecMatchLimit, kSecMatchLimitOne),
                ],
            )
        };
        let mut found: CFTypeRef = std::ptr::null();
        // Safety: the query built above, and a slot the call fills with a CFData it hands over.
        let status = unsafe { SecItemCopyMatching(asked.0, &raw mut found) };
        if status == NOT_FOUND {
            return Ok(None);
        }
        if status != SUCCESS {
            return Err(refused("read a secret", status));
        }
        let found = Owned(found);
        // Safety: a CFData, valid while `found` is held.
        let bytes = unsafe {
            let length = usize::try_from(CFDataGetLength(found.0)).unwrap_or(0);
            std::slice::from_raw_parts(CFDataGetBytePtr(found.0), length).to_vec()
        };
        Ok(Some(String::from_utf8(bytes)?))
    }

    pub(super) fn remove(place: &Place) -> Result<bool> {
        // Safety: a query built above, read by the call.
        match unsafe { SecItemDelete(query(place, &[]).0) } {
            SUCCESS => Ok(true),
            NOT_FOUND => Ok(false),
            status => Err(refused("forget a secret", status)),
        }
    }
}

/// Android: each value sealed with an AES key the Keystore holds, in the
/// app's private preferences: `org.balaur.platform.Secrets`.
#[cfg(target_os = "android")]
mod os {
    use super::{Place, Result, backends};

    pub(super) const fn name() -> &'static str {
        backends::KEYSTORE
    }

    pub(super) fn set(place: &Place, value: &str) -> Result<()> {
        anyhow::ensure!(
            crate::android::secrets::set(&place.item(), value)?,
            "the keystore refused it"
        );
        Ok(())
    }

    pub(super) fn get(place: &Place) -> Result<Option<String>> {
        Ok(crate::android::secrets::get(&place.item())?)
    }

    pub(super) fn remove(place: &Place) -> Result<bool> {
        Ok(crate::android::secrets::remove(&place.item())?)
    }
}

/// The Windows Credential Manager: one generic credential per key, named
/// after the game and the key.
#[cfg(windows)]
mod os {
    use super::{Place, Result, backends};
    use anyhow::anyhow;
    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    };

    pub(super) const fn name() -> &'static str {
        backends::CREDENTIAL_MANAGER
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn target(place: &Place) -> Vec<u16> {
        wide(&format!("{}/{}", place.service, place.key))
    }

    pub(super) fn set(place: &Place, value: &str) -> Result<()> {
        let mut target = target(place);
        let mut user = wide(&place.key);
        let mut blob = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: u32::try_from(blob.len())?,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: user.as_mut_ptr(),
            ..CREDENTIALW::default()
        };
        // Safety: every pointer in `credential` outlives the call, which copies them.
        if unsafe { CredWriteW(&raw const credential, 0) } == 0 {
            // Safety: reads this thread's last error.
            return Err(anyhow!("CredWriteW failed: error {}", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }

    pub(super) fn get(place: &Place) -> Result<Option<String>> {
        let target = target(place);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // Safety: a NUL-terminated name, and a slot the call fills.
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &raw mut found) } == 0 {
            // Safety: reads this thread's last error.
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(anyhow!("CredReadW failed: error {error}"));
        }
        // Safety: the credential the call handed over, freed once copied.
        let bytes = unsafe {
            let credential = &*found;
            let length = usize::try_from(credential.CredentialBlobSize).unwrap_or(0);
            let bytes = if credential.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(credential.CredentialBlob, length).to_vec()
            };
            CredFree(found.cast());
            bytes
        };
        Ok(Some(String::from_utf8(bytes)?))
    }

    pub(super) fn remove(place: &Place) -> Result<bool> {
        let target = target(place);
        // Safety: a NUL-terminated name.
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            // Safety: reads this thread's last error.
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(false);
            }
            return Err(anyhow!("CredDeleteW failed: error {error}"));
        }
        Ok(true)
    }
}

/// Linux: the Secret Service through libsecret's `secret-tool`, where it is
/// installed; else `name` answers the file store, said once in the log.
#[cfg(all(unix, not(any(target_vendor = "apple", target_os = "android"))))]
mod os {
    use super::{Place, Result, backends};
    use anyhow::anyhow;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    const TOOL: &str = "secret-tool";
    const LOOKUP: &str = "lookup";
    const SERVICE: &str = "service";
    const ACCOUNT: &str = "account";

    /// Whether `secret-tool` is on the `PATH` and a Secret Service answers
    /// it, found out once: a lookup of a key nobody keeps says nothing on
    /// stderr where the service runs, and why not where it does not.
    fn has_tool() -> bool {
        static HAS: OnceLock<bool> = OnceLock::new();
        *HAS.get_or_init(|| {
            let on_path = std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|folder| folder.join(TOOL).is_file())
            });
            let answers = on_path
                && Command::new(TOOL)
                    .args([LOOKUP, SERVICE, "balaur.probe", ACCOUNT, "probe"])
                    .stdin(Stdio::null())
                    .output()
                    .is_ok_and(|out| out.stderr.is_empty());
            if !answers {
                tracing::info!(
                    "secrets: no Secret Service through {TOOL} here, so secrets are kept in a file under the user directory that only this user may read"
                );
            }
            answers
        })
    }

    pub(super) fn name() -> &'static str {
        if has_tool() {
            backends::SECRET_SERVICE
        } else {
            backends::FILE
        }
    }

    fn attributes<'a>(command: &'a mut Command, place: &Place) -> &'a mut Command {
        command
            .arg(SERVICE)
            .arg(&place.service)
            .arg(ACCOUNT)
            .arg(&place.key)
    }

    pub(super) fn set(place: &Place, value: &str) -> Result<()> {
        let mut command = Command::new(TOOL);
        command
            .arg("store")
            .arg(format!("--label={} {}", place.service, place.key));
        let mut child = attributes(&mut command, place)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(value.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        anyhow::ensure!(
            out.status.success(),
            "{TOOL} store: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    }

    pub(super) fn get(place: &Place) -> Result<Option<String>> {
        let mut command = Command::new(TOOL);
        command.arg(LOOKUP);
        let out = attributes(&mut command, place)
            .stdin(Stdio::null())
            .output()?;
        // `lookup` exits 1 with nothing on stdout for a key that holds nothing.
        if !out.status.success() {
            return if out.stderr.is_empty() {
                Ok(None)
            } else {
                Err(anyhow!(
                    "{TOOL} lookup: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            };
        }
        Ok(Some(String::from_utf8(out.stdout)?))
    }

    pub(super) fn remove(place: &Place) -> Result<bool> {
        let held = get(place)?.is_some();
        let mut command = Command::new(TOOL);
        command.arg("clear");
        let out = attributes(&mut command, place)
            .stdin(Stdio::null())
            .output()?;
        anyhow::ensure!(
            out.status.success(),
            "{TOOL} clear: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(held)
    }
}

/// A page: `localStorage`, which any script on the page reads.
#[cfg(target_family = "wasm")]
mod os {
    use super::{Place, Result, backends};
    use anyhow::anyhow;

    pub(super) const fn name() -> &'static str {
        backends::LOCAL_STORAGE
    }

    fn storage() -> Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| anyhow!("this page has no localStorage"))
    }

    pub(super) fn set(place: &Place, value: &str) -> Result<()> {
        storage()?
            .set_item(&place.item(), value)
            .map_err(|_| anyhow!("localStorage refused it"))
    }

    pub(super) fn get(place: &Place) -> Result<Option<String>> {
        storage()?
            .get_item(&place.item())
            .map_err(|_| anyhow!("localStorage could not be read"))
    }

    pub(super) fn remove(place: &Place) -> Result<bool> {
        let storage = storage()?;
        let item = place.item();
        let held = storage.get_item(&item).ok().flatten().is_some();
        storage
            .remove_item(&item)
            .map_err(|_| anyhow!("localStorage refused it"))?;
        Ok(held)
    }
}
