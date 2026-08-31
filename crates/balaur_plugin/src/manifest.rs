use std::fmt;

/// The engine's own version, stamped into every manifest built here.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Bumped by hand whenever `Registry` changes shape. A dylib built against an
/// older registry is not merely out of date, it will read the wrong memory.
pub const REGISTRY_ABI: u32 = 1;

/// What a build has to agree on before its code may be loaded into this
/// process.
///
/// Rust has no stable ABI: a dylib passing Rust types across `dlopen` needs the
/// identical compiler and flags on both sides, and a mismatch is undefined
/// behaviour rather than a clean error. So the check happens before the call,
/// not after the crash.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fingerprint {
    pub rustc: String,
    pub engine: String,
    pub registry_abi: u32,
}

impl Fingerprint {
    #[must_use]
    pub fn current() -> Self {
        Self {
            rustc: rustc_version().to_string(),
            engine: ENGINE_VERSION.to_string(),
            registry_abi: REGISTRY_ABI,
        }
    }

    /// What differs from `other`, in words a person can act on. Empty means
    /// the two builds can share a process.
    #[must_use]
    pub fn differences(&self, other: &Self) -> Vec<String> {
        let mut out = Vec::new();
        if self.rustc != other.rustc {
            out.push(format!(
                "built with rustc {}, host is {}",
                other.rustc, self.rustc
            ));
        }
        if self.engine != other.engine {
            out.push(format!(
                "built for balaur {}, host is {}",
                other.engine, self.engine
            ));
        }
        if self.registry_abi != other.registry_abi {
            out.push(format!(
                "registry abi {}, host speaks {}",
                other.registry_abi, self.registry_abi
            ));
        }
        out
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rustc {} · balaur {} · abi {}",
            self.rustc, self.engine, self.registry_abi
        )
    }
}

/// Baked in at compile time by `build.rs`; falls back to the version the host
/// crate was built with when a plugin does not set it.
fn rustc_version() -> &'static str {
    option_env!("BALAUR_RUSTC_VERSION").unwrap_or("unknown")
}

/// Who a plugin is, and what it needs loaded before it.
#[derive(Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// Names of plugins that must register first. Resolved by name rather than
    /// by load order, because directory iteration is not deterministic and
    /// would leak into the simulation.
    pub requires: Vec<String>,
    pub fingerprint: Fingerprint,
}

impl Manifest {
    #[must_use]
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            requires: Vec::new(),
            fingerprint: Fingerprint::current(),
        }
    }

    #[must_use]
    pub fn requiring(mut self, names: &[&str]) -> Self {
        self.requires = names.iter().map(|n| (*n).to_string()).collect();
        self
    }
}
