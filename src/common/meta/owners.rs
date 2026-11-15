#[cfg(unix)]
mod unix {
    use std::ffi::{CStr, CString};

    fn user(id: Option<u32>, name: Option<&str>) -> Option<(u32, String)> {
        let name = name.map(CString::new).transpose().ok()?;
        let mut size = 16384;
        loop {
            let mut buffer = vec![0u8; size];
            let mut value = std::mem::MaybeUninit::<libc::passwd>::uninit();
            let mut result = std::ptr::null_mut();
            // SAFETY: buffers are live and sufficiently sized; the reentrant API
            // returns pointers into these buffers, copied before they are dropped.
            let code = unsafe {
                match id {
                    Some(id) => libc::getpwuid_r(
                        id,
                        value.as_mut_ptr(),
                        buffer.as_mut_ptr().cast(),
                        size,
                        &mut result,
                    ),
                    None => libc::getpwnam_r(
                        name.as_ref()?.as_ptr(),
                        value.as_mut_ptr(),
                        buffer.as_mut_ptr().cast(),
                        size,
                        &mut result,
                    ),
                }
            };
            if code == libc::ERANGE && size < 1024 * 1024 {
                size *= 2;
                continue;
            }
            if code != 0 || result.is_null() {
                return None;
            }
            // SAFETY: a nonnull successful result refers to initialized passwd data.
            let value = unsafe { value.assume_init() };
            let text = unsafe { CStr::from_ptr(value.pw_name) }
                .to_string_lossy()
                .into_owned();
            return Some((value.pw_uid, text));
        }
    }
    fn group(id: Option<u32>, name: Option<&str>) -> Option<(u32, String)> {
        let name = name.map(CString::new).transpose().ok()?;
        let mut size = 16384;
        loop {
            let mut buffer = vec![0u8; size];
            let mut value = std::mem::MaybeUninit::<libc::group>::uninit();
            let mut result = std::ptr::null_mut();
            // SAFETY: as for user(), the reentrant API owns no returned allocation.
            let code = unsafe {
                match id {
                    Some(id) => libc::getgrgid_r(
                        id,
                        value.as_mut_ptr(),
                        buffer.as_mut_ptr().cast(),
                        size,
                        &mut result,
                    ),
                    None => libc::getgrnam_r(
                        name.as_ref()?.as_ptr(),
                        value.as_mut_ptr(),
                        buffer.as_mut_ptr().cast(),
                        size,
                        &mut result,
                    ),
                }
            };
            if code == libc::ERANGE && size < 1024 * 1024 {
                size *= 2;
                continue;
            }
            if code != 0 || result.is_null() {
                return None;
            }
            // SAFETY: successful nonnull results initialize value and gr_name.
            let value = unsafe { value.assume_init() };
            let text = unsafe { CStr::from_ptr(value.gr_name) }
                .to_string_lossy()
                .into_owned();
            return Some((value.gr_gid, text));
        }
    }
    pub fn user_name(id: u32) -> Option<String> {
        user(Some(id), None).map(|(_, name)| name)
    }
    pub fn group_name(id: u32) -> Option<String> {
        group(Some(id), None).map(|(_, name)| name)
    }
    pub fn user_id(name: &str) -> Option<u32> {
        user(None, Some(name)).map(|(id, _)| id)
    }
    pub fn group_id(name: &str) -> Option<u32> {
        group(None, Some(name)).map(|(id, _)| id)
    }
}
#[cfg(unix)]
pub use unix::*;
#[cfg(not(unix))]
pub fn user_name(id: u32) -> Option<String> {
    (id == 0).then(|| "-".into())
}
#[cfg(not(unix))]
pub fn group_name(id: u32) -> Option<String> {
    user_name(id)
}
#[cfg(not(unix))]
pub fn user_id(name: &str) -> Option<u32> {
    (name == "-").then_some(0)
}
#[cfg(not(unix))]
pub fn group_id(name: &str) -> Option<u32> {
    user_id(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_roundtrip() {
        if let Some(name) = user_name(0) {
            assert_eq!(user_id(&name), Some(0));
        }
    }
    #[test]
    fn group_roundtrip() {
        if let Some(name) = group_name(0) {
            assert_eq!(group_id(&name), Some(0));
        }
    }
    #[test]
    fn unknown_owners() {
        assert_eq!(user_id("agentcmd-nonexistent-owner-912499"), None);
        assert_eq!(group_id("agentcmd-nonexistent-owner-912499"), None);
    }
}
