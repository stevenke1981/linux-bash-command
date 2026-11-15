//! Filesystem operations shared by the Phase 2 command shims.
pub mod link;
pub mod unlink;

use std::path::Path;

fn display(path: &Path) -> String {
    crate::common::path::display_path(path)
}

fn quote(path: &Path) -> String {
    let name = display(path);
    let quoted =
        String::from_utf8_lossy(&crate::common::escape::quote_name(name.as_bytes())).into_owned();
    if quoted == name {
        format!("'{name}'")
    } else {
        quoted
    }
}
pub mod cp;
pub mod ln;
pub mod mkdir;
mod mode;
pub mod mv;
pub mod rm;
pub mod rmdir;
pub mod touch;
mod transfer;
#[cfg(test)]
mod test_support {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    pub struct Sandbox(pub PathBuf);
    impl Sandbox {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "agentcmd-fs-unit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        pub fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        pub fn write(&self, name: &str, data: &[u8]) {
            let p = self.path(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }
        pub fn arg(&self, name: &str) -> String {
            self.path(name).to_string_lossy().into_owned()
        }
        pub fn exists(&self, name: &str) -> bool {
            Path::new(&self.path(name)).exists()
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
