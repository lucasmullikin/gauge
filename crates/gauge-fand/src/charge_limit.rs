//! Sets the charge limit built into macOS 27, the same one in System Settings. Gauge doesn't touch
//! the battery hardware, and macOS keeps enforcing the limit if this helper or Gauge isn't running.
//! This preference is private to macOS, and only root's copy counts, so it lives in this helper.

use core_foundation::base::TCFType;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::preferences::{
    CFPreferencesSetValue, CFPreferencesSynchronize, kCFPreferencesAnyHost, kCFPreferencesCurrentUser,
};
use std::ffi::c_char;

const DOMAIN: &str = "com.apple.smartcharging.topoffprotection";
const STATE_KEY: &str = "MCLFeatureState";
const LIMIT_KEY: &str = "mclLimitValue";

unsafe extern "C" {
    fn notify_post(name: *const c_char) -> u32;
}

/// The caller has already checked the range. Gauge confirms the result by asking `pmset`.
pub fn set(limit: Option<u8>) {
    let domain = CFString::new(DOMAIN);
    let write = |key: &str, value: i32| {
        let key = CFString::new(key);
        let value = CFNumber::from(value);
        // SAFETY: all objects are alive for the call, and the host and user constants are valid.
        unsafe {
            CFPreferencesSetValue(
                key.as_concrete_TypeRef(),
                value.as_CFTypeRef(),
                domain.as_concrete_TypeRef(),
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            );
        }
    };
    write(STATE_KEY, i32::from(limit.is_some()));
    if let Some(limit) = limit {
        write(LIMIT_KEY, i32::from(limit));
    }
    // SAFETY: the domain is alive, the constants are valid, and the name is NUL-terminated.
    unsafe {
        CFPreferencesSynchronize(domain.as_concrete_TypeRef(), kCFPreferencesCurrentUser, kCFPreferencesAnyHost);
        notify_post(c"com.apple.smartcharging.defaultschanged".as_ptr());
    }
}
