//! The parsers that read bytes somebody else wrote, against bytes nobody wrote.
//!
//! A pack arrives with a downloaded game, a `.glb` arrives with an asset
//! somebody was sent, and a scene is a file on disk. None of the three is the
//! engine's own output once a project leaves the machine that built it, so
//! each has to answer a corrupted input with an error rather than a panic.
//!
//! Not a fuzzer: `cargo fuzz` wants a nightly toolchain and this workspace is
//! pinned to stable. The mutations are a fixed-seed walk over a valid input,
//! so a failure here reproduces from the seed and the case number alone.

use balaur_core::Pack;

/// xorshift64*, so the corpus is the same on every machine and every run.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, limit: usize) -> usize {
        if limit == 0 {
            return 0;
        }
        (self.next() % limit as u64) as usize
    }
}

/// Every corruption a truncated download or a bad disk produces: a shorter
/// file, one byte changed, one byte inserted, one byte dropped.
fn corruptions(valid: &[u8], cases: usize) -> Vec<Vec<u8>> {
    let mut rng = Seeded(0x5eed_1234_5eed_1234);
    let mut out = Vec::with_capacity(cases + 3);
    out.push(Vec::new());
    out.push(valid[..valid.len() / 2].to_vec());
    out.push(valid[..valid.len().saturating_sub(1)].to_vec());
    for _ in 0..cases {
        let mut bytes = valid.to_vec();
        if bytes.is_empty() {
            break;
        }
        let at = rng.below(bytes.len());
        match rng.next() % 4 {
            0 => bytes.truncate(at),
            1 => bytes[at] ^= 1 << (rng.next() % 8),
            2 => bytes.insert(at, (rng.next() % 256) as u8),
            _ => {
                bytes.remove(at);
            }
        }
        out.push(bytes);
    }
    out
}

fn a_pack() -> Pack {
    let mut pack = Pack {
        manifest: String::from("[application]\nname = \"robust\"\n"),
        ..Pack::default()
    };
    pack.scenes.insert(
        String::from("main.toml"),
        String::from("[[nodes]]\nid = \"n\"\nname = \"Root\"\n"),
    );
    pack.scripts
        .insert(String::from("s.rn"), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    pack.assets
        .insert(String::from("art/a.png"), vec![0x89, b'P', b'N', b'G']);
    pack
}

#[test]
fn a_corrupted_pack_is_an_error_and_never_a_panic() {
    let valid = a_pack().encode();
    assert!(
        Pack::decode(&valid).is_ok(),
        "the corpus itself must decode"
    );
    for (case, bytes) in corruptions(&valid, 2000).into_iter().enumerate() {
        // The contract is "no panic", so the result is deliberately unused:
        // a pack that still decodes after a flipped byte is fine, and one
        // that refuses is the point.
        let decoded = std::panic::catch_unwind(|| Pack::decode(&bytes));
        assert!(
            decoded.is_ok(),
            "case {case}: Pack::decode panicked on {} bytes",
            bytes.len()
        );
    }
}

/// A `.glb` is a container with its own lengths, which is where a reader
/// trusting the file walks off the end of it.
#[test]
fn a_corrupted_glb_is_an_error_and_never_a_panic() {
    // 12-byte header, then one JSON chunk holding the smallest legal document.
    let json = br#"{"asset":{"version":"2.0"},"meshes":[],"nodes":[],"scenes":[]}"#;
    let padded = json.len().next_multiple_of(4);
    let mut chunk = json.to_vec();
    chunk.resize(padded, b' ');
    let mut valid = Vec::new();
    valid.extend_from_slice(b"glTF");
    valid.extend_from_slice(&2u32.to_le_bytes());
    valid.extend_from_slice(&((12 + 8 + padded) as u32).to_le_bytes());
    valid.extend_from_slice(&(padded as u32).to_le_bytes());
    valid.extend_from_slice(b"JSON");
    valid.extend_from_slice(&chunk);

    for (case, bytes) in corruptions(&valid, 2000).into_iter().enumerate() {
        let read = std::panic::catch_unwind(|| {
            balaur_core::glb::parse_gltf(&bytes, "corrupt.glb", &|_| {
                Err(anyhow::anyhow!("no side files"))
            })
        });
        assert!(
            read.is_ok(),
            "case {case}: parse_gltf panicked on {} bytes",
            bytes.len()
        );
    }
}
