//! A file on the machine's disk written whole: what is written lands in a
//! file of this writer's own beside the target, `<name>.part.<pid>.<count>`,
//! and takes the target's place once it is all there and on the disk, the
//! folder synced after. A crash, a power cut or a second writer never leaves
//! half a file, or another writer's, under the name a reader trusts, and a
//! write that fails or is let go leaves nothing beside it. What a script
//! writes goes through the files backend instead
//! ([`crate::files::FileBackend::write_whole`]), named as this names it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// What follows a target's whole name in the name of a file written for it.
const PARTIAL: &str = ".part.";

/// `target` with `suffix` after its whole name: `a.json` and `a.toml` never
/// share a file beside them.
#[must_use]
pub fn beside(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The file a write to `target` goes through, its own: the target's whole
/// name, the process and a count, so no writer reuses another's.
#[must_use]
pub fn partial_of(target: &Path) -> PathBuf {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let count = WRITES.fetch_add(1, Ordering::Relaxed);
    beside(target, &format!("{PARTIAL}{}.{count}", process()))
}

/// This process, among those that may write beside the same file: a
/// browser tab draws one, as it has no process and `std::process::id`
/// panics there.
fn process() -> u32 {
    #[cfg(not(target_family = "wasm"))]
    return std::process::id();
    #[cfg(target_family = "wasm")]
    {
        static TAB: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a draw in [0, 1) spread over the u32s"
        )]
        *TAB.get_or_init(|| (js_sys::Math::random() * f64::from(u32::MAX)) as u32)
    }
}

/// How long a file a write went through may sit beside its target before
/// another write takes it for one a writer that was killed left.
const STALE: std::time::Duration = std::time::Duration::from_mins(10);

/// What writers killed partway left beside `target` and nobody has touched
/// for [`STALE`]: a writer still at it has written lately.
fn sweep_stale(target: &Path) {
    let Some(name) = target.file_name() else {
        return;
    };
    let prefix = format!("{}{PARTIAL}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(folder_of(target)) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|idle| idle > STALE);
        if stale && entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The folder `target` is in, `.` for a bare name.
fn folder_of(target: &Path) -> &Path {
    target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Every file a write to `target` went through and left, by a writer that
/// stopped partway: `<name>.part.<pid>.<count>`, or `<stem>.part`, the name
/// a writer that knew the stem alone took. Only a caller that holds the
/// target's lock may sweep, as no other writer has one open then.
pub fn sweep(target: &Path) {
    let _ = std::fs::remove_file(target.with_extension("part"));
    let Some(name) = target.file_name() else {
        return;
    };
    let prefix = format!("{}{PARTIAL}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(folder_of(target)) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Options that make a file readable and writable by this user alone, as a
/// secret is kept; elsewhere than unix, the folder it is in decides.
#[must_use]
pub fn private_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

/// `path` made readable by this user alone, where it is there and its
/// folder keeps modes: one that keeps none, a FAT drive, fails nothing.
pub fn make_private(path: &Path) {
    #[cfg(unix)]
    match std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            tracing::debug!("{} kept its mode: {err}", path.display());
        }
        _ => {}
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The file a write goes through, removed when it is let go unless the
/// write kept it.
struct Partial(PathBuf);

impl Partial {
    fn keep(mut self) {
        self.0 = PathBuf::new();
    }
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// A file being written whole to `target`: write to it, then [`Whole::finish`].
/// Let go before then, what it wrote is removed and the target is as it was.
pub struct Whole {
    // Closed before the file it writes is removed: fields drop in order.
    file: std::fs::File,
    partial: Partial,
    target: PathBuf,
}

impl Whole {
    /// A write to `target`, its folder made where it is not there.
    ///
    /// # Errors
    /// Where the folder or the file it goes through cannot be made.
    pub fn create(target: &Path) -> std::io::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        let whole = Self::open(target, options)?;
        // The mode the file had, which a move over it would otherwise drop.
        if let Ok(meta) = std::fs::metadata(&whole.target) {
            whole.file.set_permissions(meta.permissions())?;
        }
        Ok(whole)
    }

    /// A write to `target` readable and writable by this user alone.
    ///
    /// # Errors
    /// Where the folder or the file it goes through cannot be made.
    pub fn private(target: &Path) -> std::io::Result<Self> {
        Self::open(target, private_options())
    }

    fn open(target: &Path, mut options: std::fs::OpenOptions) -> std::io::Result<Self> {
        // A link stays a link: what it names is written whole.
        let target = match std::fs::symlink_metadata(target) {
            Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(target)?,
            _ => target.to_path_buf(),
        };
        std::fs::create_dir_all(folder_of(&target))?;
        options.create_new(true);
        // Made new, never one that was there, so its mode is the one asked
        // for; a name a writer killed in an earlier run left is passed over.
        let mut tries = 0;
        let (partial, file) = loop {
            let partial = partial_of(&target);
            match options.open(&partial) {
                Ok(file) => break (partial, file),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && tries < 8 => {
                    tries += 1;
                }
                Err(err) => return Err(err),
            }
        };
        Ok(Self {
            file,
            partial: Partial(partial),
            target,
        })
    }

    /// The file the bytes go through until they are whole.
    #[must_use]
    pub fn partial(&self) -> &Path {
        &self.partial.0
    }

    /// The target takes `mode` once it is whole, as a program that runs
    /// does, `0o755`; nothing where a file has no mode.
    ///
    /// # Errors
    /// Where the mode cannot be set.
    pub fn set_mode(&self, mode: u32) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            self.file
                .set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        Ok(())
    }

    /// What was written put in the target's place, on the disk: the bytes
    /// synced, moved over the target, and the folder synced, which a folder
    /// that keeps no such thing fails nothing for. What writers killed
    /// partway left beside it long ago is swept.
    ///
    /// # Errors
    /// Where the bytes cannot be synced or moved; nothing is left beside the
    /// target then.
    pub fn finish(self) -> std::io::Result<()> {
        self.file.sync_all()?;
        let target = self.target.clone();
        self.put()?;
        sync_folder(&target);
        sweep_stale(&target);
        Ok(())
    }

    /// What was written put in the target's place unsynced: for a cache that
    /// checks what it reads, which a power cut may cost a rebuild.
    ///
    /// # Errors
    /// Where the bytes cannot be moved; nothing is left beside the target then.
    pub fn finish_cached(self) -> std::io::Result<()> {
        self.put()
    }

    fn put(self) -> std::io::Result<()> {
        let Self {
            file,
            partial,
            target,
        } = self;
        // Closed before it takes the target's place.
        close(file);
        std::fs::rename(&partial.0, &target)?;
        partial.keep();
        Ok(())
    }
}

/// A file closed, which it is as it is let go.
fn close(_file: std::fs::File) {}

impl Write for Whole {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// `bytes` written whole to `target`, on the disk.
///
/// # Errors
/// Where the file cannot be made, written, synced or moved; the target is as
/// it was then.
pub fn write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut whole = Whole::create(target)?;
    whole.write_all(bytes)?;
    whole.finish()
}

/// `bytes` written whole to `target` unsynced, as a cache keeps them
/// ([`Whole::finish_cached`]).
///
/// # Errors
/// Where the file cannot be made, written or moved; the target is as it was
/// then.
pub fn write_cached(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut whole = Whole::create(target)?;
    whole.write_all(bytes)?;
    whole.finish_cached()
}

/// The folder `target` is in synced, so a move into it lasts a power cut;
/// a folder that cannot be synced, as on Windows, fails nothing.
fn sync_folder(target: &Path) {
    #[cfg(unix)]
    if let Err(err) = std::fs::File::open(folder_of(target)).and_then(|folder| folder.sync_all()) {
        tracing::debug!("the folder of {} was not synced: {err}", target.display());
    }
    #[cfg(not(unix))]
    let _ = target;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(folder: &Path) -> Vec<String> {
        std::fs::read_dir(folder)
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect()
    }

    #[test]
    fn each_write_goes_through_a_file_of_its_own_named_after_the_whole_target() {
        let json = partial_of(Path::new("keys.json"));
        let toml = partial_of(Path::new("keys.toml"));
        assert!(
            json.to_string_lossy().starts_with("keys.json.part."),
            "{json:?}"
        );
        assert!(
            toml.to_string_lossy().starts_with("keys.toml.part."),
            "{toml:?}"
        );
        assert_ne!(
            json,
            partial_of(Path::new("keys.json")),
            "two writes share a file"
        );
    }

    #[test]
    fn a_write_finished_takes_the_place_and_one_let_go_leaves_nothing() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("deep/save.toml");
        write(&target, b"one").unwrap();
        let mut let_go = Whole::create(&target).unwrap();
        let_go.write_all(b"half").unwrap();
        drop(let_go);
        assert_eq!(std::fs::read(&target).unwrap(), b"one");
        assert_eq!(listed(target.parent().unwrap()), ["save.toml"]);
    }

    #[test]
    fn writers_at_once_each_land_whole() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("shared.bin");
        std::thread::scope(|scope| {
            for writer in 0..4u8 {
                let target = &target;
                scope.spawn(move || {
                    for _ in 0..10 {
                        write(target, &[writer; 4096]).unwrap();
                    }
                });
            }
        });
        let landed = std::fs::read(&target).unwrap();
        assert_eq!(landed.len(), 4096);
        assert!(
            landed.iter().all(|byte| *byte == landed[0]),
            "two writes mixed"
        );
        assert_eq!(listed(home.path()), ["shared.bin"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_private_write_is_this_user_s_alone_and_a_mode_asked_for_holds() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let secret = home.path().join("secret.json");
        let whole = Whole::private(&secret).unwrap();
        assert_eq!(
            mode(whole.partial()),
            0o600,
            "readable by others while it is written"
        );
        whole.finish().unwrap();
        assert_eq!(mode(&secret), 0o600);
        let program = home.path().join("run");
        let whole = Whole::create(&program).unwrap();
        whole.set_mode(0o755).unwrap();
        whole.finish().unwrap();
        assert_eq!(mode(&program), 0o755);
    }

    #[test]
    fn a_sweep_takes_what_stopped_writes_left_and_nothing_else() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("secrets.json");
        for name in [
            "secrets.json",
            "secrets.json.part.7.0",
            "secrets.part",
            "secrets.jsonl",
        ] {
            std::fs::write(home.path().join(name), "x").unwrap();
        }
        sweep(&target);
        let mut left = listed(home.path());
        left.sort();
        assert_eq!(left, ["secrets.json", "secrets.jsonl"]);
    }

    #[test]
    fn a_name_an_earlier_run_left_is_passed_over() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("slot.toml");
        // The name the next write would take, left by a writer killed earlier.
        let taken = partial_of(&target).to_string_lossy().into_owned();
        let (stem, count) = taken.rsplit_once('.').unwrap();
        let next = format!("{stem}.{}", count.parse::<u64>().unwrap() + 1);
        std::fs::write(&next, "left").unwrap();
        write(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn what_a_killed_writer_left_long_ago_is_swept_and_a_writer_at_it_is_not() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("pack.zip");
        let old = home.path().join("pack.zip.part.9.0");
        let fresh = home.path().join("pack.zip.part.9.1");
        for left in [&old, &fresh] {
            std::fs::write(left, "half").unwrap();
        }
        let long_ago = std::time::UNIX_EPOCH;
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        write(&target, b"whole").unwrap();
        assert!(!old.exists(), "a killed writer's file was left");
        assert!(fresh.exists(), "a writer still at it lost its file");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_stays_a_link_and_a_file_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("real.toml");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = home.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write(&link, b"new").unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        let mode = std::fs::metadata(&real).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640, "the file lost its mode");
    }
}
