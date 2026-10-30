//! Owner and group lookups for find's `-user`, `-group`, `-nouser` and `-nogroup` tests.
//!
//! On Unix the account databases are read with `getpwnam`, `getpwuid`, `getgrnam` and
//! `getgrgid`. Windows has no such databases: every file reports uid and gid 0 and the name
//! `-` (see `docs/spec` 02-conventions §7), so `-user -` and `-group -` match there.

use std::fs::Metadata;

/// The numeric user id for a user name, or `None` if the name is unknown.
#[cfg(unix)]
pub fn user_id(name: &str) -> Option<u32> {
    let name = std::ffi::CString::new(name).ok()?;
    // SAFETY: `name` is a valid NUL-terminated string. The returned pointer refers to static
    // storage that is read immediately, before any other account lookup can reuse it.
    unsafe {
        let entry = libc::getpwnam(name.as_ptr());
        (!entry.is_null()).then(|| (*entry).pw_uid)
    }
}

/// The numeric group id for a group name, or `None` if the name is unknown.
#[cfg(unix)]
pub fn group_id(name: &str) -> Option<u32> {
    let name = std::ffi::CString::new(name).ok()?;
    // SAFETY: as for `user_id`; the group database entry is read immediately.
    unsafe {
        let entry = libc::getgrnam(name.as_ptr());
        (!entry.is_null()).then(|| (*entry).gr_gid)
    }
}

#[cfg(unix)]
fn user_exists(uid: u32) -> bool {
    // SAFETY: getpwuid takes a plain integer and returns a pointer or null; only null is tested.
    unsafe { !libc::getpwuid(uid as libc::uid_t).is_null() }
}

#[cfg(unix)]
fn group_exists(gid: u32) -> bool {
    // SAFETY: as for `user_exists`.
    unsafe { !libc::getgrgid(gid as libc::gid_t).is_null() }
}

/// Windows: the single account is `-` with id 0, so it is always named.
#[cfg(not(unix))]
pub fn user_id(name: &str) -> Option<u32> {
    (name == "-").then_some(0)
}

/// Windows: see [`user_id`].
#[cfg(not(unix))]
pub fn group_id(name: &str) -> Option<u32> {
    (name == "-").then_some(0)
}

#[cfg(not(unix))]
fn user_exists(_uid: u32) -> bool {
    true
}

#[cfg(not(unix))]
fn group_exists(_gid: u32) -> bool {
    true
}

/// True when the file's owner id has no account entry (`-nouser`).
pub fn has_no_user(meta: &Metadata) -> bool {
    !user_exists(crate::common::meta::uid_gid(meta).0)
}

/// True when the file's group id has no group entry (`-nogroup`).
pub fn has_no_group(meta: &Metadata) -> bool {
    !group_exists(crate::common::meta::uid_gid(meta).1)
}

/// The owner id of a file (`-uid`), 0 on Windows.
pub fn owner_uid(meta: &Metadata) -> u32 {
    crate::common::meta::uid_gid(meta).0
}

/// The group id of a file (`-gid`), 0 on Windows.
pub fn owner_gid(meta: &Metadata) -> u32 {
    crate::common::meta::uid_gid(meta).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_ids_of_the_current_owner_resolve_to_a_name() {
        let meta = std::fs::metadata(".").unwrap();
        assert!(!has_no_user(&meta));
        assert!(!has_no_group(&meta));
    }

    #[test]
    fn unknown_names_do_not_resolve() {
        assert_eq!(user_id("agentcmd-no-such-user-xyz"), None);
        assert_eq!(group_id("agentcmd-no-such-group-xyz"), None);
    }
}
