//! Build-generated trust metadata for the optional owner-waived private beta.
//!
//! Manifest data does not grant this trust: `build.rs` emits these pins only
//! when a separately staged resource is present at compile time.

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrivateBetaTrust {
    pub(crate) pack_id: &'static str,
    pub(crate) version: &'static str,
    pub(crate) manifest_sha256: &'static str,
    pub(crate) bundle_sha256: &'static str,
    pub(crate) item_ids: &'static [&'static str],
    pub(crate) published: bool,
}

include!(concat!(env!("OUT_DIR"), "/private_beta_trust.rs"));
