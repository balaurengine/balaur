//! A macOS runtime a test can bundle: a real Mach-O on a Mac, which signs
//! what it bundles, built for the OS the templates are.

use std::path::Path;

/// The OS `scripts/package.sh` builds the macOS template for.
const TEMPLATE_MACOS: u32 = 12 << 16;
const FAT_MAGIC: u32 = 0xcafe_babe;
const LC_BUILD_VERSION: u32 = 0x32;

/// `/usr/bin/true` at `path`, claiming the template's macOS rather than the
/// one this machine's system was built for; elsewhere, bytes no signer reads.
pub(crate) fn runtime(path: &Path) {
    if !cfg!(target_os = "macos") {
        std::fs::write(path, [0xca, 0xfe, 0xba, 0xbe]).unwrap();
        return;
    }
    let mut bin = std::fs::read("/usr/bin/true").unwrap();
    let big = |bin: &[u8], at: usize| u32::from_be_bytes(bin[at..at + 4].try_into().unwrap());
    let slices: Vec<usize> = if big(&bin, 0) == FAT_MAGIC {
        (0..big(&bin, 4) as usize)
            .map(|slice| big(&bin, 8 + slice * 20 + 8) as usize)
            .collect()
    } else {
        vec![0]
    };
    for start in slices {
        let word = |bin: &[u8], at: usize| {
            u32::from_le_bytes(bin[start + at..start + at + 4].try_into().unwrap())
        };
        let mut at = 32;
        for _ in 0..word(&bin, 16) {
            if word(&bin, at) == LC_BUILD_VERSION {
                let minos = start + at + 12;
                bin[minos..minos + 4].copy_from_slice(&TEMPLATE_MACOS.to_le_bytes());
            }
            at += word(&bin, at + 4) as usize;
        }
    }
    std::fs::write(path, bin).unwrap();
}
