// A game's `.bs` scripts as native code: their C, built for the target with
// Xcode for an Apple target on a Mac that has it, or with clang and lld for
// any, and put where the game loads it from (`standalone::native_scripts_beside`).

use crate::apple::AppleConfig;
use crate::config::NativeScripts;
use crate::vocabulary::words;
use anyhow::{Context, Result, bail};
use balaur::standalone::{native_scripts_beside, native_scripts_file};
use std::path::{Path, PathBuf};
use std::process::Command;

const AARCH64: &str = "aarch64";
const ARM64: &str = "arm64";
const BIN: &str = "bin";
const CLANG: &str = "clang";
const LLD_LINK: &str = "lld-link";

/// Every build's flags: the C needs no library, and contracting a multiply
/// and an add into one rounding would part its floats from every engine's.
const FLAGS: [&str; 6] = [
    "-O2",
    "-ffreestanding",
    "-fno-math-errno",
    "-fno-stack-protector",
    "-ffp-contract=off",
    "-w",
];

/// Where LLVM installs itself beyond `PATH`: Homebrew keeps lld apart from
/// clang, and Windows has one directory, under whichever Program Files the
/// machine has.
fn known_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/opt/homebrew/opt/llvm/bin",
        "/opt/homebrew/opt/lld/bin",
        "/usr/local/opt/llvm/bin",
        "/usr/local/opt/lld/bin",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(programs) = std::env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(programs).join("LLVM").join(BIN));
    }
    dirs
}

/// The API floor every 64-bit Android ABI shares; the C calls nothing that
/// a later one adds.
const ANDROID_API: u32 = 21;

/// What an export asked of native scripts: their C, and whether a missing
/// compiler fails it.
pub(crate) struct Request {
    pub scripts: balaur::NativeScripts,
    pub mode: NativeScripts,
    pub min_macos: String,
}

/// The [`Request`] an export makes, or `None` when the scripts stay on the
/// target's engine: `off`, no `.bs` script, or a target that runs no native code.
pub(crate) fn request(
    opts: &crate::Options<'_>,
    config: &crate::ExportConfig,
    extra: &mut [Box<dyn balaur_plugin::Plugin>],
    pack: &balaur::Pack,
    apple: &AppleConfig,
) -> Result<Option<Request>> {
    let (mode, project) = (config.native_scripts, opts.path.as_path());
    let wanted = opts
        .target
        .as_deref()
        .is_some_and(|t| crate::split_variant(t).0 != words::targets::WEB);
    if mode == NativeScripts::Off || !wanted {
        return Ok(None);
    }
    let Some(scripts) = balaur::native_scripts(project, extra, pack)? else {
        return Ok(None);
    };
    Ok(Some(Request {
        scripts,
        mode,
        min_macos: apple.min_macos.clone(),
    }))
}

/// clang, and the lld for each object format, each found on its own.
#[derive(Debug, Default)]
pub(crate) struct Llvm {
    clang: Option<PathBuf>,
    ld64: Option<PathBuf>,
    elf: Option<PathBuf>,
    coff: Option<PathBuf>,
}

impl Llvm {
    pub(crate) fn find() -> Self {
        Self::find_in(&search_dirs())
    }

    fn find_in(dirs: &[PathBuf]) -> Self {
        let tool = |name: &str| {
            let file = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            };
            dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file())
        };
        Self {
            clang: tool(CLANG),
            ld64: tool("ld64.lld"),
            elf: tool("ld.lld"),
            coff: tool(LLD_LINK),
        }
    }

    /// clang and `linker`, or why the scripts cannot be built here.
    fn with<'a>(
        &'a self,
        linker: Option<&'a Path>,
        name: &str,
    ) -> Result<(&'a Path, &'a Path), String> {
        match (self.clang.as_deref(), linker) {
            (Some(clang), Some(link)) => Ok((clang, link)),
            (None, _) => Err("no clang was found".into()),
            (_, None) => Err(format!("no {name} was found")),
        }
    }
}

/// `BALAUR_LLVM` first, then `PATH`, then where LLVM installs itself, and
/// Debian's `llvm-N` directories newest first.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("BALAUR_LLVM")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(known_dirs());
    let mut versioned: Vec<(u32, PathBuf)> = std::fs::read_dir("/usr/lib")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let version = name.strip_prefix("llvm-")?.parse().ok()?;
                    Some((version, e.path().join(BIN)))
                })
                .collect()
        })
        .unwrap_or_default();
    versioned.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    dirs.extend(versioned.into_iter().map(|(_, dir)| dir));
    dirs
}

/// Without what a target needs: an error under `on`, else a note that the
/// scripts stay on the target's engine.
fn missing(request: &Request, why: &str) -> Result<Option<PathBuf>> {
    if request.mode == NativeScripts::On {
        bail!(
            "[export] native_scripts = \"on\", but {why}: install clang and lld, or name \
             their directory in BALAUR_LLVM"
        );
    }
    tracing::info!(
        "{} script(s) run on the target's engine, not as native code: {why}",
        request.scripts.modules
    );
    Ok(None)
}

/// A scratch directory for one build, removed when it goes.
struct Work(PathBuf);

impl Work {
    fn new(scripts: &balaur::NativeScripts) -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("balaur-native-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let work = Self(dir);
        std::fs::write(work.source(), &scripts.c)
            .with_context(|| format!("writing {}", work.source().display()))?;
        Ok(work)
    }

    fn source(&self) -> PathBuf {
        self.0.join("scripts.c")
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let out = cmd
        .output()
        .with_context(|| format!("{what}: running {}", cmd.get_program().to_string_lossy()))?;
    if !out.status.success() {
        bail!("{what} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

fn compile(clang: &Path, work: &Work, triple: &str) -> Result<PathBuf> {
    let obj = work.file(&format!("{triple}.o"));
    let mut cmd = Command::new(clang);
    cmd.arg(format!("--target={triple}")).args(FLAGS);
    if !triple.contains(words::native::WINDOWS) {
        cmd.arg("-fPIC");
    }
    run(
        cmd.arg("-c").arg("-o").arg(&obj).arg(work.source()),
        "compiling the scripts' C",
    )?;
    Ok(obj)
}

fn link_elf(ld: &Path, obj: &Path, out: &Path, android: bool) -> Result<()> {
    let mut cmd = Command::new(ld);
    cmd.args(["-shared", "--no-undefined", "-soname"])
        .arg(native_scripts_file(words::native::LINUX));
    // An Android device with 16 KB pages loads nothing aligned to less.
    if android {
        cmd.args(["-z", "max-page-size=16384"]);
    }
    run(cmd.arg("-o").arg(out).arg(obj), "linking the scripts")
}

fn link_coff(link: &Path, obj: &Path, out: &Path, arch: &str) -> Result<()> {
    // lld-link writes an import library beside its output; only the DLL ships.
    let staged = obj.with_extension(words::extensions::DLL);
    let machine = if arch == words::targets::X86_64 {
        "x64"
    } else {
        ARM64
    };
    run(
        Command::new(link)
            .args(["/dll", "/noentry", "/nodefaultlib"])
            .arg(format!("/machine:{machine}"))
            .arg(format!("/out:{}", staged.display()))
            .arg(obj),
        "linking the scripts",
    )?;
    std::fs::copy(&staged, out).with_context(|| format!("writing {}", out.display()))?;
    Ok(())
}

/// One Apple slice: `platform` and `min` as `ld64.lld -platform_version` takes them.
struct AppleSlice<'a> {
    arch: &'a str,
    triple: String,
    sdk: &'a str,
    platform: &'a str,
    min: &'a str,
}

fn apple_slice(
    llvm: &Llvm,
    work: &Work,
    slice: &AppleSlice<'_>,
    install: &str,
    out: &Path,
) -> Result<Option<String>> {
    if has_xcode(slice.sdk) {
        run(
            Command::new(words::native::XCRUN)
                .args(["--sdk", slice.sdk, CLANG, "-target", &slice.triple])
                .args(FLAGS)
                .args(["-fPIC", "-dynamiclib", "-install_name", install, "-o"])
                .arg(out)
                .arg(work.source()),
            "compiling the scripts with Xcode",
        )?;
        return Ok(None);
    }
    let (clang, ld64) = match llvm.with(llvm.ld64.as_deref(), "ld64.lld") {
        Ok(tools) => tools,
        Err(why) => return Ok(Some(format!("no Xcode here, and {why}"))),
    };
    let obj = compile(clang, work, &slice.triple)?;
    // lld 14 signs a macOS slice of its own accord and an iOS one only when
    // asked, and the simulator loads nothing unsigned.
    run(
        Command::new(ld64)
            .args([
                "-dylib",
                "-adhoc_codesign",
                "-arch",
                slice.arch,
                "-platform_version",
                slice.platform,
                slice.min,
                slice.min,
            ])
            .args(["-install_name", install, "-o"])
            .arg(out)
            .arg(&obj),
        "linking the scripts",
    )?;
    Ok(None)
}

fn has_xcode(sdk: &str) -> bool {
    cfg!(target_os = "macos")
        && Command::new(words::native::XCRUN)
            .args(["--sdk", sdk, "--show-sdk-path"])
            .output()
            .is_ok_and(|o| o.status.success())
}

/// Thin Mach-O files as one universal file, each slice at a 16 KiB boundary.
fn universal(slices: &[Vec<u8>]) -> Result<Vec<u8>> {
    const ALIGN: u32 = 14;
    let at = |n: usize| n.div_ceil(1 << ALIGN) * (1 << ALIGN);
    let mut out = Vec::new();
    out.extend(0xcafe_babe_u32.to_be_bytes());
    out.extend(u32::try_from(slices.len())?.to_be_bytes());
    let mut offset = at(8 + 20 * slices.len());
    let mut placed = Vec::with_capacity(slices.len());
    for slice in slices {
        let word = |i: usize| {
            slice
                .get(i..i + 4)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_le_bytes)
        };
        if word(0) != Some(MH_MAGIC_64) {
            bail!("a slice that is not a 64-bit Mach-O");
        }
        for w in [
            word(4).unwrap_or_default(),
            word(8).unwrap_or_default(),
            u32::try_from(offset)?,
            u32::try_from(slice.len())?,
            ALIGN,
        ] {
            out.extend(w.to_be_bytes());
        }
        placed.push(offset);
        offset = at(offset + slice.len());
    }
    for (slice, start) in slices.iter().zip(placed) {
        out.resize(start, 0);
        out.extend(slice);
    }
    Ok(out)
}

/// The scripts beside a desktop game's executable, built for `target`: the
/// file written, for a signature to cover, or `None` when it stays on its engine.
pub(crate) fn build_desktop(
    exe: &Path,
    target: &str,
    request: &Request,
) -> Result<Option<PathBuf>> {
    let base = crate::split_variant(target).0;
    let llvm = Llvm::find();
    let work = Work::new(&request.scripts)?;
    let (os, built) = match base {
        words::targets::LINUX_X64 | words::targets::LINUX_ARM64 => {
            let arch = if base == words::targets::LINUX_X64 {
                words::targets::X86_64
            } else {
                AARCH64
            };
            let (clang, ld) = match llvm.with(llvm.elf.as_deref(), "ld.lld") {
                Ok(tools) => tools,
                Err(why) => return missing(request, &why),
            };
            let obj = compile(clang, &work, &format!("{arch}-linux-gnu"))?;
            let out = native_scripts_beside(exe, words::native::LINUX);
            link_elf(ld, &obj, &out, false)?;
            (words::native::LINUX, out)
        }
        words::targets::WINDOWS_X64 | words::targets::WINDOWS_ARM64 => {
            let arch = if base == words::targets::WINDOWS_X64 {
                words::targets::X86_64
            } else {
                AARCH64
            };
            let (clang, link) = match llvm.with(llvm.coff.as_deref(), LLD_LINK) {
                Ok(tools) => tools,
                Err(why) => return missing(request, &why),
            };
            let obj = compile(clang, &work, &format!("{arch}-pc-windows-msvc"))?;
            let out = native_scripts_beside(exe, words::native::WINDOWS);
            link_coff(link, &obj, &out, arch)?;
            (words::native::WINDOWS, out)
        }
        words::targets::MACOS_UNIVERSAL => {
            let out = native_scripts_beside(exe, words::native::MACOS);
            let install = format!("@rpath/{}", native_scripts_file(words::native::MACOS));
            let mut slices = Vec::new();
            for arch in [ARM64, "x86_64"] {
                let slice = AppleSlice {
                    arch,
                    triple: format!("{arch}-apple-macos{}", request.min_macos),
                    sdk: "macosx",
                    platform: words::native::MACOS,
                    min: &request.min_macos,
                };
                let file = work.file(&format!("{arch}.dylib"));
                if let Some(why) = apple_slice(&llvm, &work, &slice, &install, &file)? {
                    return missing(request, &why);
                }
                slices.push(std::fs::read(&file)?);
            }
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&out, universal(&slices)?)
                .with_context(|| format!("writing {}", out.display()))?;
            (words::native::MACOS, out)
        }
        _ => return Ok(None),
    };
    tracing::info!(
        "{} script(s) as native code for {os} -> {}",
        request.scripts.modules,
        built.display()
    );
    Ok(Some(built))
}

/// The scripts in each 64-bit ABI an Android layout carries, as
/// `lib/<abi>/libBalaurScripts.so`; the 32-bit ones keep their engine.
pub(crate) fn build_android(layout: &Path, request: &Request) -> Result<()> {
    let lib = layout.join(words::android::LIB);
    let mut abis: Vec<String> = std::fs::read_dir(&lib)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    abis.sort();
    let llvm = Llvm::find();
    let work = Work::new(&request.scripts)?;
    for abi in abis {
        let arch = match abi.as_str() {
            words::targets::ARM64_V8A => AARCH64,
            words::targets::X86_64 => words::targets::X86_64,
            other => {
                tracing::info!("{other} has no native scripts; its scripts run on its engine");
                continue;
            }
        };
        let (clang, ld) = match llvm.with(llvm.elf.as_deref(), "ld.lld") {
            Ok(tools) => tools,
            Err(why) => return missing(request, &why).map(|_| ()),
        };
        let obj = compile(clang, &work, &format!("{arch}-linux-android{ANDROID_API}"))?;
        let out = lib
            .join(&abi)
            .join(native_scripts_file(words::targets::ANDROID));
        link_elf(ld, &obj, &out, true)?;
        tracing::info!(
            "{} script(s) as native code for {abi} -> {}",
            request.scripts.modules,
            out.display()
        );
    }
    Ok(())
}

/// What a template's executable was built for, read off its Mach-O header.
#[derive(Debug, PartialEq, Eq)]
struct Slice {
    arch: &'static str,
    simulator: bool,
}

const MH_MAGIC_64: u32 = 0xfeed_facf;
const CPU_ARM64: u32 = 0x0100_000c;
const CPU_X86_64: u32 = 0x0100_0007;
const LC_BUILD_VERSION: u32 = 0x32;
const PLATFORM_IOS_SIMULATOR: u32 = 7;

fn slice_of(bin: &[u8]) -> Option<Slice> {
    let word = |at: usize| Some(u32::from_le_bytes(bin.get(at..at + 4)?.try_into().ok()?));
    if word(0)? != MH_MAGIC_64 {
        return None;
    }
    let arch = match word(4)? {
        CPU_ARM64 => ARM64,
        CPU_X86_64 => words::targets::X86_64,
        _ => return None,
    };
    let mut at = 32;
    for _ in 0..word(16)? {
        let (cmd, size) = (word(at)?, word(at + 4)? as usize);
        if cmd == LC_BUILD_VERSION {
            let simulator = word(at + 8)? == PLATFORM_IOS_SIMULATOR;
            return Some(Slice { arch, simulator });
        }
        at += size;
    }
    Some(Slice {
        arch,
        simulator: false,
    })
}

/// The scripts as `<app>/Frameworks/BalaurScripts.framework`, for the device
/// or the simulator the template's executable was built for.
pub(crate) fn build_framework(
    app: &Path,
    executable: &str,
    request: &Request,
    apple: &AppleConfig,
) -> Result<()> {
    let bin = std::fs::read(app.join(executable))
        .with_context(|| format!("reading the runtime {executable}"))?;
    let Some(slice) = slice_of(&bin) else {
        bail!("{executable} is not an iOS executable this exporter can read");
    };
    let (sdk, platform, plist_platform, suffix) = if slice.simulator {
        (
            "iphonesimulator",
            "ios-simulator",
            "iPhoneSimulator",
            "-simulator",
        )
    } else {
        ("iphoneos", words::targets::IOS, "iPhoneOS", "")
    };
    let file = native_scripts_beside(&app.join(executable), words::targets::IOS);
    let dir = file.parent().context("a framework with no directory")?;
    let work = Work::new(&request.scripts)?;
    let staged = work.file("framework");
    let ios = AppleSlice {
        arch: slice.arch,
        triple: format!("{}-apple-ios{}{suffix}", slice.arch, apple.min_ios),
        sdk,
        platform,
        min: &apple.min_ios,
    };
    let install = format!("@rpath/{}", native_scripts_file(words::targets::IOS));
    if let Some(why) = apple_slice(&Llvm::find(), &work, &ios, &install, &staged)? {
        return missing(request, &why).map(|_| ());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::copy(&staged, &file).with_context(|| format!("writing {}", file.display()))?;
    let name = balaur::standalone::NATIVE_SCRIPTS;
    std::fs::write(
        dir.join("Info.plist"),
        apple.framework_plist(name, plist_platform)?,
    )?;
    tracing::info!(
        "{} script(s) as native code for iOS -> {}",
        request.scripts.modules,
        dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(cpu: u32, platform: Option<u32>) -> Vec<u8> {
        let mut b = vec![];
        for w in [
            super::MH_MAGIC_64,
            cpu,
            0,
            2,
            u32::from(platform.is_some()),
            24,
            0,
            0,
        ] {
            b.extend(w.to_le_bytes());
        }
        if let Some(p) = platform {
            for w in [super::LC_BUILD_VERSION, 24, p, 0, 0, 0] {
                b.extend(w.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_template_says_whether_it_is_for_the_simulator() {
        assert_eq!(
            slice_of(&header(super::CPU_ARM64, Some(7))),
            Some(Slice {
                arch: "arm64",
                simulator: true
            })
        );
        assert_eq!(
            slice_of(&header(super::CPU_ARM64, Some(2))),
            Some(Slice {
                arch: "arm64",
                simulator: false
            })
        );
        assert_eq!(slice_of(b"#!/bin/sh"), None);
    }

    fn request(mode: NativeScripts) -> Request {
        Request {
            scripts: balaur::NativeScripts {
                framework: balaur::standalone::NATIVE_SCRIPTS,
                c: String::new(),
                modules: 3,
            },
            mode,
            min_macos: "11.0".into(),
        }
    }

    #[test]
    fn a_universal_file_holds_each_slice_at_its_own_aligned_offset() {
        let a = [header(CPU_ARM64, None), vec![1; 100]].concat();
        let b = [header(CPU_X86_64, None), vec![2; 50]].concat();
        let fat = universal(&[a.clone(), b.clone()]).unwrap();
        let word = |at: usize| u32::from_be_bytes(fat[at..at + 4].try_into().unwrap());
        assert_eq!((word(0), word(4)), (0xcafe_babe, 2));
        for (i, slice) in [&a, &b].into_iter().enumerate() {
            let entry = 8 + 20 * i;
            let (offset, size) = (word(entry + 8) as usize, word(entry + 12) as usize);
            assert_eq!(offset % (1 << 14), 0);
            assert_eq!(&fat[offset..offset + size], slice.as_slice());
        }
        assert_eq!(word(8), CPU_ARM64);
        assert!(universal(&[b"#!/bin/sh".to_vec()]).is_err());
    }

    #[test]
    fn each_tool_is_found_in_whichever_directory_holds_it() {
        let (llvm, lld) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let exe = |name: &str| {
            if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.into()
            }
        };
        std::fs::write(llvm.path().join(exe("clang")), "").unwrap();
        std::fs::write(lld.path().join(exe("ld.lld")), "").unwrap();
        let found = Llvm::find_in(&[llvm.path().into(), lld.path().into()]);
        assert_eq!(found.clang, Some(llvm.path().join(exe("clang"))));
        assert_eq!(found.elf, Some(lld.path().join(exe("ld.lld"))));
        assert!(found.with(found.elf.as_deref(), "ld.lld").is_ok());
        assert_eq!(
            found.with(found.coff.as_deref(), "lld-link").unwrap_err(),
            "no lld-link was found"
        );
    }

    #[test]
    fn without_a_compiler_auto_falls_back_and_on_fails() {
        assert!(
            missing(&request(NativeScripts::Auto), "no clang was found")
                .unwrap()
                .is_none()
        );
        let err = missing(&request(NativeScripts::On), "no clang was found").unwrap_err();
        assert!(err.to_string().contains("BALAUR_LLVM"), "{err}");
    }

    // The real thing, where clang and lld are installed: hello's scripts for
    // every target, each library needing nothing, and this machine's loaded.
    #[test]
    fn every_target_builds_hello_needing_nothing() {
        let llvm = Llvm::find();
        if llvm.clang.is_none() || llvm.ld64.is_none() || llvm.elf.is_none() || llvm.coff.is_none()
        {
            eprintln!("no clang and lld here; the native build is not exercised");
            return;
        }
        let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
        let pack = balaur::build_pack(&hello).unwrap();
        let scripts = balaur::native_scripts(&hello, &mut [], &pack)
            .unwrap()
            .unwrap();
        let request = Request {
            scripts,
            mode: NativeScripts::On,
            min_macos: "11.0".into(),
        };
        let out = tempfile::tempdir().unwrap();
        let magic = |path: &Path| std::fs::read(path).unwrap()[..4].to_vec();
        // Side by side: each is a clang and a link of its own, and one after
        // another they were the longest test in the workspace.
        std::thread::scope(|scope| {
            for (target, start) in [
                ("linux-x64", b"\x7fELF".to_vec()),
                ("linux-arm64-server", b"\x7fELF".to_vec()),
                ("windows-x64", b"MZ".to_vec()),
                ("windows-arm64", b"MZ".to_vec()),
                ("macos-universal", vec![0xca, 0xfe, 0xba, 0xbe]),
            ] {
                let (out, request, magic) = (&out, &request, &magic);
                scope.spawn(move || {
                    let exe = out.path().join(target).join("game");
                    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
                    let lib = build_desktop(&exe, target, request).unwrap().unwrap();
                    assert_eq!(magic(&lib)[..start.len()], start, "{target}");
                });
            }
        });
        assert_eq!(
            build_desktop(&out.path().join("game"), "web", &request).unwrap(),
            None
        );

        let layout = out.path().join("android");
        for abi in ["arm64-v8a", "x86_64", "armeabi-v7a"] {
            std::fs::create_dir_all(layout.join("lib").join(abi)).unwrap();
        }
        build_android(&layout, &request).unwrap();
        for abi in ["arm64-v8a", "x86_64"] {
            assert_eq!(
                magic(&layout.join("lib").join(abi).join("libBalaurScripts.so")),
                b"\x7fELF"
            );
        }
        assert!(!layout.join("lib/armeabi-v7a/libBalaurScripts.so").exists());

        let app = out.path().join("Run.app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Run"), header(CPU_ARM64, Some(2))).unwrap();
        let apple = AppleConfig {
            bundle_id: "org.balaur.hello".into(),
            ..AppleConfig::default()
        };
        build_framework(&app, "Run", &request, &apple).unwrap();
        let framework = app.join("Frameworks/BalaurScripts.framework");
        assert_eq!(
            magic(&framework.join("BalaurScripts")),
            MH_MAGIC_64.to_le_bytes()
        );
        assert!(framework.join("Info.plist").is_file());

        // Where there is no Xcode, as in a Linux container: lld links the slice.
        let work = Work::new(&request.scripts).unwrap();
        let device = AppleSlice {
            arch: "arm64",
            triple: "arm64-apple-ios15.0".into(),
            sdk: "no-such-sdk",
            platform: "ios",
            min: "15.0",
        };
        let linked = out.path().join("lld-ios");
        assert_eq!(
            apple_slice(&llvm, &work, &device, "@rpath/x", &linked).unwrap(),
            None
        );
        assert_eq!(
            slice_of(&std::fs::read(&linked).unwrap()),
            Some(Slice {
                arch: "arm64",
                simulator: false
            })
        );

        let host = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", _) => Some("macos-universal"),
            ("linux", "x86_64") => Some("linux-x64"),
            ("linux", "aarch64") => Some("linux-arm64"),
            _ => None,
        };
        if let Some(target) = host {
            let lib =
                native_scripts_beside(&out.path().join(target).join("game"), std::env::consts::OS);
            // Each script's unit, and the one the project's instance starts from.
            assert_eq!(
                balaur::script_bs::load_native(&lib).unwrap(),
                request.scripts.modules + 1
            );
        }
    }
}
