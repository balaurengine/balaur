//! A file on the machine's disk written whole: what is written lands in a
//! file of this writer's own beside the target, `<name>.part.<pid>.<count>`,
//! and takes the target's place once it is all there and on the disk, the
//! folder synced after. A crash, a power cut or a second writer never leaves
//! half a file, or another writer's, under the name a reader trusts, and a
//! write that fails or is let go leaves nothing beside it. What a script
//! writes goes through the files backend instead
//! ([`crate::files::FileBackend::write`]), named as this names it.

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
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|idle| idle > STALE);
        if stale {
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
        if let Some(meta) = std::fs::metadata(&whole.target)
            .ok()
            .filter(|_| !whole.in_place())
        {
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
        // A device or a pipe, `/dev/stdout`, is written as it stands: it holds
        // no file to keep whole, nor a folder to make one beside it in.
        if std::fs::metadata(&target).is_ok_and(|meta| !meta.is_file() && !meta.is_dir()) {
            return Ok(Self {
                file: std::fs::OpenOptions::new().write(true).open(&target)?,
                partial: Partial(PathBuf::new()),
                target,
            });
        }
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
        if let Some(folder) = self.land(Sync::Full)? {
            sync_dir(&folder);
        }
        Ok(())
    }

    /// Whether the target is a device or a pipe, written as it stands.
    fn in_place(&self) -> bool {
        self.partial.0.as_os_str().is_empty()
    }

    /// The bytes synced as `sync` says and put in the target's place, and
    /// what writers killed partway left beside it long ago swept; the folder
    /// to sync for the move to last, none for a device.
    fn land(self, sync: Sync) -> std::io::Result<Option<PathBuf>> {
        if self.in_place() {
            self.put()?;
            return Ok(None);
        }
        sync.file(&self.file)?;
        let target = self.target.clone();
        self.put()?;
        sweep_stale(&target);
        Ok(Some(folder_of(&target).to_path_buf()))
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
        if !partial.0.as_os_str().is_empty() {
            std::fs::rename(&partial.0, &target)?;
        }
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

/// An archive writer goes back over what it wrote, as a zip's index does.
impl std::io::Seek for Whole {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(to)
    }
}

/// `bytes` written whole to `target`, on the disk.
///
/// # Errors
/// Where the file cannot be made, written, synced or moved; the target is as
/// it was then.
pub fn write(target: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut whole = Whole::create(target.as_ref())?;
    whole.write_all(bytes.as_ref())?;
    whole.finish()
}

/// `bytes` written whole to `target` unsynced, as a cache keeps them
/// ([`Whole::finish_cached`]).
///
/// # Errors
/// Where the file cannot be made, written or moved; the target is as it was
/// then.
pub fn write_cached(target: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut whole = Whole::create(target.as_ref())?;
    whole.write_all(bytes.as_ref())?;
    whole.finish_cached()
}

/// `from` copied whole to `to` with its mode, as `std::fs::copy` copies, on
/// the disk; how many bytes.
///
/// # Errors
/// Where `from` cannot be read or `to` made, written, synced or moved; `to`
/// is as it was then.
pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<u64> {
    let (whole, copied) = copied(from.as_ref(), to.as_ref())?;
    whole.finish()?;
    Ok(copied)
}

/// [`copy`] unsynced: for an output a run makes again, a build's.
///
/// # Errors
/// Where `from` cannot be read or `to` made, written or moved; `to` is as it
/// was then.
pub fn copy_cached(from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<u64> {
    let (whole, copied) = copied(from.as_ref(), to.as_ref())?;
    whole.finish_cached()?;
    Ok(copied)
}

/// `from`'s bytes and mode in a write to `to`, file to file so the system
/// copies them as it copies a file.
fn copied(from: &Path, to: &Path) -> std::io::Result<(Whole, u64)> {
    let mut source = std::fs::File::open(from)?;
    let mut whole = Whole::create(to)?;
    whole
        .file
        .set_permissions(source.metadata()?.permissions())?;
    let copied = std::io::copy(&mut source, &mut whole.file)?;
    Ok((whole, copied))
}

/// How a file's bytes are seen onto the disk before it takes its target's
/// place.
#[derive(Clone, Copy)]
enum Sync {
    /// On the device, the drive's own cache flushed.
    Full,
    /// Ordered ahead of the move that follows, on a Mac: a power cut leaves
    /// the old file or the new one whole, never one cut short, and a full
    /// sync after the last write of a step makes them all last. Elsewhere,
    /// as [`Sync::Full`].
    Ordered,
}

impl Sync {
    fn file(self, file: &std::fs::File) -> std::io::Result<()> {
        #[cfg(target_vendor = "apple")]
        if matches!(self, Self::Ordered) {
            use std::os::fd::AsRawFd as _;
            // SAFETY: a barrier asked of a file this holds open; it reads and
            // writes no memory.
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_BARRIERFSYNC) } != -1 {
                return Ok(());
            }
        }
        let _ = self;
        file.sync_all()
    }
}

/// Files written whole in one step: each through a file of its own and
/// moved over its target, and each folder they went into synced once, as
/// the batch is let go or [`finish`](Batch::finish)es, where [`write`]
/// syncs it each time: a rename's rewrites, a formatter's sources, a
/// fetched folder. On a Mac each file is ordered ahead of its move rather
/// than flushed, and the folders' syncs flush them all.
#[derive(Default)]
pub struct Batch {
    folders: std::collections::BTreeSet<PathBuf>,
}

impl Batch {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `bytes` written whole to `target`, its folder synced with the rest.
    ///
    /// # Errors
    /// Where the file cannot be made, written, synced or moved; the target is
    /// as it was then.
    pub fn write(
        &mut self,
        target: impl AsRef<Path>,
        bytes: impl AsRef<[u8]>,
    ) -> std::io::Result<()> {
        self.made_for(target.as_ref());
        let mut whole = Whole::create(target.as_ref())?;
        whole.write_all(bytes.as_ref())?;
        self.folders.extend(whole.land(Sync::Ordered)?);
        Ok(())
    }

    /// `from` copied whole to `to` with its mode, as [`copy`] copies it, its
    /// folder synced with the rest; how many bytes.
    ///
    /// # Errors
    /// Where `from` cannot be read or `to` made, written, synced or moved;
    /// `to` is as it was then.
    pub fn copy(&mut self, from: impl AsRef<Path>, to: impl AsRef<Path>) -> std::io::Result<u64> {
        self.made_for(to.as_ref());
        let (whole, copied) = copied(from.as_ref(), to.as_ref())?;
        self.folders.extend(whole.land(Sync::Ordered)?);
        Ok(copied)
    }

    /// The folders a write to `target` is about to make, and the one that
    /// holds the first of them, kept to sync: a new folder lasts once the
    /// folder it was made in is synced.
    fn made_for(&mut self, target: &Path) {
        let mut folder = folder_of(target);
        while !folder.exists() {
            self.folders.insert(folder.to_path_buf());
            let Some(parent) = folder.parent().filter(|p| !p.as_os_str().is_empty()) else {
                return;
            };
            folder = parent;
        }
        if folder != folder_of(target) {
            self.folders.insert(folder.to_path_buf());
        }
    }

    /// The step's end: every folder written into synced, once each, the
    /// first that cannot be its error; on a Mac, the flush that makes the
    /// step's files last.
    ///
    /// # Errors
    /// Where a folder cannot be opened or synced.
    pub fn finish(mut self) -> std::io::Result<()> {
        for folder in std::mem::take(&mut self.folders) {
            #[cfg(unix)]
            std::fs::File::open(&folder)?.sync_all()?;
            #[cfg(not(unix))]
            let _ = folder;
        }
        Ok(())
    }
}

/// A batch let go before its end, an error's early return, still syncs what
/// it wrote, as far as it can.
impl Drop for Batch {
    fn drop(&mut self) {
        for folder in &self.folders {
            sync_dir(folder);
        }
    }
}

/// `folder` synced; one that cannot be, as on Windows, fails nothing.
fn sync_dir(folder: &Path) {
    #[cfg(unix)]
    if let Err(err) = std::fs::File::open(folder).and_then(|opened| opened.sync_all()) {
        tracing::debug!("{} was not synced: {err}", folder.display());
    }
    #[cfg(not(unix))]
    let _ = folder;
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
                        write(target, [writer; 4096]).unwrap();
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

    #[test]
    fn a_copy_lands_whole_with_its_mode_and_nothing_beside_it() {
        let home = tempfile::tempdir().unwrap();
        let from = home.path().join("runtime");
        std::fs::write(&from, b"the program").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let to = home.path().join("out/game");
        assert_eq!(copy_cached(&from, &to).unwrap(), 11);
        assert_eq!(std::fs::read(&to).unwrap(), b"the program");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&to).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "the copy lost its mode");
        }
        assert_eq!(listed(to.parent().unwrap()), ["game"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_device_is_written_as_it_stands() {
        write("/dev/null", b"nothing kept").unwrap();
        let meta = std::fs::metadata("/dev/null").unwrap();
        assert!(!meta.is_file(), "the device was replaced by a file");
    }

    #[test]
    fn a_batch_lands_each_file_whole_in_every_folder() {
        let home = tempfile::tempdir().unwrap();
        let mut batch = Batch::new();
        for (folder, name) in [("a", "one.bs"), ("a", "two.bs"), ("b/c", "three.bs")] {
            batch
                .write(home.path().join(folder).join(name), name)
                .unwrap();
        }
        let program = home.path().join("tools/run");
        let from = home.path().join("run");
        std::fs::write(&from, b"#!").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(batch.copy(&from, &program).unwrap(), 2);
        batch.finish().unwrap();
        assert_eq!(
            std::fs::read(home.path().join("b/c/three.bs")).unwrap(),
            b"three.bs"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&program).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "the copy lost its mode");
        }
        let mut a = listed(&home.path().join("a"));
        a.sort();
        assert_eq!(a, ["one.bs", "two.bs"]);
    }
}
