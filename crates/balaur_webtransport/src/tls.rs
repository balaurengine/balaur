//! Certificates, because QUIC has no plaintext mode.
//!
//! Two cases, and they are genuinely different. A shipped server has a real
//! certificate from a real authority and clients trust it through the system
//! roots, as they would any website. Two engines on one machine have neither,
//! and setting one up would be absurd, so the server generates a throwaway
//! and the client pins it by hash.
//!
//! Pinning by hash is also what a browser accepts, through
//! `serverCertificateHashes` — but only for a certificate valid for at most
//! two weeks, which is why the generated one is short-lived rather than
//! long-lived out of convenience.

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// How long a generated certificate lasts. Inside the two weeks a browser
/// will accept for a hash-pinned certificate, so the same one works there.
const DAYS_VALID: i64 = 13;

/// A certificate chain and its private key.
pub struct Certificate {
    pub(crate) chain: Vec<CertificateDer<'static>>,
    pub(crate) key: PrivateKeyDer<'static>,
}

impl Certificate {
    /// Generate a short-lived self-signed certificate for `names`.
    ///
    /// # Errors
    /// When the key or certificate cannot be generated.
    #[allow(
        clippy::disallowed_methods,
        reason = "a certificate's validity is wall-clock by definition, and never enters the simulation"
    )]
    pub fn self_signed(names: &[&str]) -> Result<Self> {
        let mut params = rcgen::CertificateParams::new(
            names.iter().map(|n| (*n).to_string()).collect::<Vec<_>>(),
        )
        .context("building certificate parameters")?;
        let now = std::time::SystemTime::now();
        params.not_before = now.into();
        params.not_after =
            (now + std::time::Duration::from_secs(60 * 60 * 24 * DAYS_VALID as u64)).into();
        let key = rcgen::KeyPair::generate().context("generating a key pair")?;
        let certificate = params
            .self_signed(&key)
            .context("self-signing the certificate")?;
        Ok(Self {
            chain: vec![certificate.der().clone()],
            key: PrivateKeyDer::try_from(key.serialize_der())
                .map_err(|e| anyhow::anyhow!("the generated key is unusable: {e}"))?,
        })
    }

    /// A certificate and key already on disk, as DER.
    ///
    /// # Errors
    /// When the key is not a form rustls accepts.
    pub fn from_der(chain: Vec<Vec<u8>>, key: Vec<u8>) -> Result<Self> {
        Ok(Self {
            chain: chain.into_iter().map(CertificateDer::from).collect(),
            key: PrivateKeyDer::try_from(key)
                .map_err(|e| anyhow::anyhow!("the private key is unusable: {e}"))?,
        })
    }

    /// The SHA-256 of each certificate, which is what a client pins.
    #[must_use]
    pub fn hashes(&self) -> Vec<Vec<u8>> {
        self.chain
            .iter()
            .map(|der| {
                ring::digest::digest(&ring::digest::SHA256, der.as_ref())
                    .as_ref()
                    .to_vec()
            })
            .collect()
    }

    /// A copy of the key, since rustls's type is deliberately not `Clone`.
    pub(crate) fn key(&self) -> PrivateKeyDer<'static> {
        self.key.clone_key()
    }
}
