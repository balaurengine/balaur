//! The OS a Mach-O was built for, read off its load commands: what a macOS
//! template's binary will not start below, since a flat runtime carries no
//! `Info.plist` to say so.

/// A universal binary's header, big-endian on disk.
const FAT_MAGIC: u32 = 0xcafe_babe;
/// A 64-bit slice's header, little-endian on every architecture shipped.
const MH_MAGIC_64: u32 = 0xfeed_facf;
/// Where a slice's load commands start, after its header.
const LOAD_COMMANDS: usize = 32;
/// `fat_arch`: five words a slice, its offset the third.
const FAT_ARCH: usize = 20;
const FAT_OFFSET: usize = 8;
/// The command naming the platform and the minimum OS, and the older one
/// a Mac binary built before it carried.
const LC_BUILD_VERSION: u32 = 0x32;
const LC_VERSION_MIN_MACOSX: u32 = 0x24;
const LC_VERSION_MIN_IPHONEOS: u32 = 0x25;

/// The lowest OS every slice of `bin` starts on, as `(major, minor)`; `None`
/// for bytes that are not a Mach-O this reads, or one that names no minimum.
pub(crate) fn minimum_os(bin: &[u8]) -> Option<(u32, u32)> {
    let big = |at: usize| Some(u32::from_be_bytes(bin.get(at..at + 4)?.try_into().ok()?));
    if big(0)? != FAT_MAGIC {
        return slice_minimum(bin);
    }
    (0..big(4)? as usize)
        .map(|slice| {
            let offset = big(8 + slice * FAT_ARCH + FAT_OFFSET)? as usize;
            slice_minimum(bin.get(offset..)?)
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()
}

fn slice_minimum(bin: &[u8]) -> Option<(u32, u32)> {
    let word = |at: usize| Some(u32::from_le_bytes(bin.get(at..at + 4)?.try_into().ok()?));
    if word(0)? != MH_MAGIC_64 {
        return None;
    }
    let mut at = LOAD_COMMANDS;
    for _ in 0..word(16)? {
        let (command, size) = (word(at)?, word(at + 4)? as usize);
        let version = match command {
            // platform, then minos
            LC_BUILD_VERSION => word(at + 12)?,
            LC_VERSION_MIN_MACOSX | LC_VERSION_MIN_IPHONEOS => word(at + 8)?,
            _ => {
                at += size.max(8);
                continue;
            }
        };
        // xxxx.yy.zz, the patch never deciding anything here.
        return Some((version >> 16, (version >> 8) & 0xff));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{FAT_MAGIC, LC_BUILD_VERSION, MH_MAGIC_64, minimum_os};

    /// A 64-bit slice with one `LC_BUILD_VERSION` naming `major.minor`.
    fn slice(major: u32, minor: u32) -> Vec<u8> {
        let mut bin = Vec::new();
        for word in [MH_MAGIC_64, 0x0100_000c, 0, 2, 1, 24, 0, 0] {
            bin.extend_from_slice(&word.to_le_bytes());
        }
        for word in [LC_BUILD_VERSION, 24, 1, (major << 16) | (minor << 8), 0, 0] {
            bin.extend_from_slice(&word.to_le_bytes());
        }
        bin
    }

    #[test]
    fn a_slice_names_the_os_it_was_built_for() {
        assert_eq!(minimum_os(&slice(12, 0)), Some((12, 0)));
        assert_eq!(minimum_os(&slice(15, 4)), Some((15, 4)));
        assert_eq!(minimum_os(b"\x7fELF"), None);
    }

    // A universal binary starts where its newest slice does.
    #[test]
    fn a_universal_binary_starts_where_its_newest_slice_does() {
        let (arm, intel) = (slice(12, 0), slice(13, 1));
        let mut fat = Vec::new();
        for word in [FAT_MAGIC, 2] {
            fat.extend_from_slice(&word.to_be_bytes());
        }
        let first = 8 + 2 * 20;
        for (offset, size) in [(first, arm.len()), (first + arm.len(), intel.len())] {
            for word in [0, 0, offset as u32, size as u32, 0] {
                fat.extend_from_slice(&word.to_be_bytes());
            }
        }
        fat.extend_from_slice(&arm);
        fat.extend_from_slice(&intel);
        assert_eq!(minimum_os(&fat), Some((13, 1)));
    }
}
