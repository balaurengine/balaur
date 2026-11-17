// Modules as native code through this machine's C compiler, as an iOS export
// builds them with Xcode's. Each library is kept under the target's temp dir by
// a hash of its C, the compiler and the flags: a rerun compiles nothing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

/// Contracting a multiply and an add into one rounding would part a float
/// from every engine's.
pub(crate) const FLAGS: [&str; 5] = ["-O2", "-ffp-contract=off", "-fPIC", "-shared", "-w"];

/// The most modules one library holds. clang compiles a file on one core: the
/// corpus as one file was 11 s of it, so the libraries build side by side.
pub(crate) const CHUNK: usize = 16;

/// The most wasm one library holds: about 200 KB of C, under a second of
/// clang. Its `any` code makes a module of `anys/` 80 KB of C.
const WASM_PER_LIBRARY: usize = 16 << 10;

/// A library no run has opened for a week is removed.
const UNUSED: Duration = Duration::from_hours(7 * 24);

/// `$CC`, else `cc`, and what it says its version is.
pub(crate) fn compiler() -> &'static (String, String) {
    static CC: OnceLock<(String, String)> = OnceLock::new();
    CC.get_or_init(|| {
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        let version = version(&cc).unwrap_or_default();
        (cc, version)
    })
}

/// What `cc --version` prints, or `None` with no such compiler.
pub(crate) fn version(cc: &str) -> Option<String> {
    let out = Command::new(cc).arg("--version").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `modules` as native code, loaded: libraries of at most [`CHUNK`] modules
/// and [`WASM_PER_LIBRARY`] bytes, built side by side; how many modules the
/// libraries hold.
pub(crate) fn load(modules: &[&[u8]]) -> Result<usize, String> {
    let mut chunks: Vec<&[&[u8]]> = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (at, wasm) in modules.iter().enumerate() {
        if at > start && (at - start == CHUNK || bytes + wasm.len() > WASM_PER_LIBRARY) {
            chunks.push(&modules[start..at]);
            (start, bytes) = (at, 0);
        }
        bytes += wasm.len();
    }
    if start < modules.len() {
        chunks.push(&modules[start..]);
    }
    let built = super::on_every_core(&mut chunks, |chunk| library(chunk));
    let mut loaded = 0;
    for lib in built {
        loaded += balaur_bs::runtime::load_native(&lib?)?;
    }
    Ok(loaded)
}

/// One library of `modules`, built, or the one an earlier run built.
fn library(modules: &[&[u8]]) -> Result<PathBuf, String> {
    let c = balaur_bs::c::translate(modules)?;
    let (cc, version) = compiler();
    kept(&[cc, version, &FLAGS.join(" "), &c], "lib", |out| {
        let source = out.with_extension("c");
        std::fs::write(&source, &c).map_err(|e| e.to_string())?;
        let built = run(Command::new(cc).args(FLAGS).arg("-o").arg(out).arg(&source));
        let _ = std::fs::remove_file(&source);
        built.map(|_| ())
    })
}

/// `cmd` run: what it printed, or why it failed.
pub(crate) fn run(cmd: &mut Command) -> Result<String, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| format!("no `{program}`: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "`{program}` failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// What `make` writes to the path it is handed, kept under a hash of `key`.
/// It is made under a name of its own and renamed into place, so a run that
/// finds the file never reads half of one.
pub(crate) fn kept(
    key: &[&str],
    extension: &str,
    make: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let dir = dir();
    let name = balaur_bs::module_hash(key.join("\0").as_bytes());
    let path = dir.join(format!("{name}.{extension}"));
    if touch(&path) {
        return Ok(path);
    }
    let staged = dir.join(format!(
        "{name}-{}-{}.{extension}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    let placed = make(&staged).and_then(|()| {
        // Windows refuses to replace a library another run has open, and
        // what that run made is this one's to the byte.
        std::fs::rename(&staged, &path).or_else(|e| {
            if path.is_file() {
                Ok(())
            } else {
                Err(format!("keeping {}: {e}", path.display()))
            }
        })
    });
    let _ = std::fs::remove_file(&staged);
    placed.map(|()| path)
}

/// Whether `path` is there, marked as used now.
#[allow(
    clippy::disallowed_methods,
    reason = "a cache file's age, outside any simulation"
)]
fn touch(path: &Path) -> bool {
    std::fs::File::open(path).is_ok_and(|f| {
        let _ = f.set_modified(SystemTime::now());
        true
    })
}

/// Where libraries are kept, emptied of those unused for [`UNUSED`].
#[allow(
    clippy::disallowed_methods,
    reason = "a cache file's age, outside any simulation"
)]
fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bs-native");
        let _ = std::fs::create_dir_all(&dir);
        let now = SystemTime::now();
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > UNUSED));
            if old {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        dir
    })
}
