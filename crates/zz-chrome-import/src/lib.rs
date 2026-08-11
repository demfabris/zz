//! Read-only import of Google Chrome profiles, cookies, and browsing history,
//! including the platform keychain and DPAPI storage-key handling.

pub mod cookie;
mod fs_util;
pub mod history;
pub mod profiles;
