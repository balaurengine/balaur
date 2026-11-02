//! Corrupted copies of a valid input, for the tests that hold a parser of
//! bytes somebody else wrote to an error rather than a panic. Shared by path:
//! each crate's suite reads the same corpus.

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
pub(crate) fn corruptions(valid: &[u8], cases: usize) -> Vec<Vec<u8>> {
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
