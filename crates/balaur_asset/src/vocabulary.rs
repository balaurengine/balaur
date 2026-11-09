//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const ID: &str = "id";
    pub(crate) const TYPE: &str = "type";
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {

    pub(crate) mod glb {
        pub(crate) const LINEAR: &str = "linear";
    }
}
