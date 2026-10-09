//! The iOS ssh identity, kept in the Keychain.

use std::fmt;

use core_foundation::{
    base::{CFType, CFTypeRef, OSStatus, TCFType as _},
    boolean::CFBoolean,
    data::CFData,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
#[cfg(test)]
use security_framework_sys::keychain_item::SecItemDelete;
use security_framework_sys::{
    access_control::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
    base::{errSecDuplicateItem, errSecItemNotFound, errSecSuccess},
    item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecReturnData,
        kSecValueData,
    },
    keychain_item::{SecItemAdd, SecItemCopyMatching, SecItemUpdate},
};
use zeroize::Zeroizing;

const SERVICE: &str = "dev.zz.ios.ssh";
const ACCOUNT: &str = "id_ed25519";

/// `errSecMissingEntitlement`, which `security-framework-sys` does not export.
const ERR_SEC_MISSING_ENTITLEMENT: OSStatus = -34018;

#[allow(
    unsafe_code,
    reason = "the accessibility key has no binding in security-framework-sys"
)]
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    /// `kSecAttrAccessible`, which `security-framework-sys` does not export.
    static kSecAttrAccessible: CFStringRef;
}

#[derive(Debug)]
pub(crate) enum KeychainError {
    Unavailable,
    Malformed,
    Status(OSStatus),
}

impl KeychainError {
    fn from_status(status: OSStatus) -> Self {
        if status == ERR_SEC_MISSING_ENTITLEMENT {
            Self::Unavailable
        } else {
            Self::Status(status)
        }
    }
}

impl fmt::Display for KeychainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("this build has no keychain entitlement"),
            Self::Malformed => formatter.write_str("the stored item is not an OpenSSH key"),
            Self::Status(status) => write!(formatter, "Security.framework status {status}"),
        }
    }
}

#[allow(
    unsafe_code,
    reason = "SecItemCopyMatching is a raw Security.framework entry point"
)]
pub(crate) fn load_identity() -> Result<Option<Zeroizing<String>>, KeychainError> {
    let mut query = identity_query();
    // SAFETY: `kSecReturnData` is a framework-owned constant, so the get rule applies.
    query.push((
        unsafe { CFString::wrap_under_get_rule(kSecReturnData) },
        CFBoolean::true_value().into_CFType(),
    ));
    let query = CFDictionary::from_CFType_pairs(&query);

    let mut found: CFTypeRef = std::ptr::null();
    // SAFETY: `query` outlives the call and is only read; `found` is a live local the call fills
    // with a +1 reference.
    let status = unsafe { SecItemCopyMatching(query.as_concrete_TypeRef(), &raw mut found) };
    if status == errSecItemNotFound {
        return Ok(None);
    }
    if status != errSecSuccess {
        return Err(KeychainError::from_status(status));
    }
    if found.is_null() {
        return Err(KeychainError::Malformed);
    }

    // SAFETY: `SecItemCopyMatching` hands back an owned reference on success, so the create rule
    // applies.
    let value = unsafe { CFType::wrap_under_create_rule(found) };
    let data = value
        .downcast_into::<CFData>()
        .ok_or(KeychainError::Malformed)?;
    let text = String::from_utf8(data.bytes().to_vec()).map_err(|_| KeychainError::Malformed)?;
    Ok(Some(Zeroizing::new(text)))
}

#[allow(
    unsafe_code,
    reason = "SecItemAdd and SecItemUpdate are raw Security.framework entry points"
)]
pub(crate) fn store_identity(identity: &str) -> Result<(), KeychainError> {
    let mut attributes = identity_query();
    attributes.extend(identity_value(identity));
    let attributes = CFDictionary::from_CFType_pairs(&attributes);

    // SAFETY: `attributes` outlives the call and is only read; a null result pointer is the
    // documented "hand nothing back".
    let status = unsafe { SecItemAdd(attributes.as_concrete_TypeRef(), std::ptr::null_mut()) };
    if status == errSecSuccess {
        return Ok(());
    }
    if status != errSecDuplicateItem {
        return Err(KeychainError::from_status(status));
    }

    let query = CFDictionary::from_CFType_pairs(&identity_query());
    let update = CFDictionary::from_CFType_pairs(&identity_value(identity));
    // SAFETY: both dictionaries outlive the call and are only read by it.
    let status =
        unsafe { SecItemUpdate(query.as_concrete_TypeRef(), update.as_concrete_TypeRef()) };
    if status == errSecSuccess {
        Ok(())
    } else {
        Err(KeychainError::from_status(status))
    }
}

#[allow(
    unsafe_code,
    reason = "the kSec* constants are framework-owned CFStrings"
)]
fn identity_query() -> Vec<(CFString, CFType)> {
    // SAFETY: every one of these is a framework-owned static CFString, so the get rule applies.
    unsafe {
        vec![
            (
                CFString::wrap_under_get_rule(kSecClass),
                CFString::wrap_under_get_rule(kSecClassGenericPassword).into_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecAttrService),
                CFString::from(SERVICE).into_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecAttrAccount),
                CFString::from(ACCOUNT).into_CFType(),
            ),
        ]
    }
}

#[cfg(test)]
#[allow(
    unsafe_code,
    reason = "SecItemDelete is a raw Security.framework entry point"
)]
fn delete_identity() -> Result<(), KeychainError> {
    let query = CFDictionary::from_CFType_pairs(&identity_query());
    // SAFETY: `query` outlives the call and the framework only reads it.
    let status = unsafe { SecItemDelete(query.as_concrete_TypeRef()) };
    if status == errSecSuccess || status == errSecItemNotFound {
        Ok(())
    } else {
        Err(KeychainError::from_status(status))
    }
}

#[allow(
    unsafe_code,
    reason = "the kSec* constants are framework-owned CFStrings"
)]
fn identity_value(identity: &str) -> Vec<(CFString, CFType)> {
    // SAFETY: as in `identity_query` — framework-owned statics, get rule.
    unsafe {
        vec![
            (
                CFString::wrap_under_get_rule(kSecValueData),
                CFData::from_buffer(identity.as_bytes()).into_CFType(),
            ),
            (
                CFString::wrap_under_get_rule(kSecAttrAccessible),
                CFString::wrap_under_get_rule(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)
                    .into_CFType(),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Option<String> {
        load_identity()
            .expect("the keychain is readable")
            .map(|identity| identity.to_string())
    }

    #[test]
    fn stores_and_loads_the_identity() {
        let previous = load_identity().expect("the keychain is readable");

        store_identity("first").expect("adding an item");
        assert_eq!(stored().as_deref(), Some("first"));

        store_identity("second").expect("updating the item");
        assert_eq!(stored().as_deref(), Some("second"));

        match previous {
            Some(previous) => store_identity(&previous).expect("restoring the real identity"),
            None => delete_identity().expect("removing the test item"),
        }
    }
}
