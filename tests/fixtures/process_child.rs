//! Minimal owned process fixture. Compiled in the test sandbox, independently
//! of the command library, so process tests need not copy the whole test runner.
use std::{fs, path::PathBuf, process::Command, thread, time::Duration};
fn main() {
    let Ok(mode) = std::env::var("AGENTCMD_P4_MODE") else {
        return;
    };
    let ready = PathBuf::from(std::env::var_os("AGENTCMD_P4_READY").unwrap());
    if mode == "exit7" {
        std::process::exit(7);
    }
    if mode == "leaf" {
        thread::sleep(Duration::from_secs(2));
        fs::write(ready, b"survived").unwrap();
        return;
    }
    let mut child = if mode == "tree" {
        Some(
            Command::new(std::env::current_exe().unwrap())
                .env("AGENTCMD_P4_MODE", "leaf")
                .env("AGENTCMD_P4_READY", ready.with_extension("leaf"))
                .spawn()
                .unwrap(),
        )
    } else {
        None
    };
    #[cfg(unix)]
    if mode == "ignore" {
        unsafe extern "C" {
            fn signal(number: i32, handler: usize) -> usize;
        }
        unsafe {
            signal(15, 1);
        }
    }
    fs::write(ready, b"ready").unwrap();
    thread::sleep(if mode == "ignore" {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(10)
    });
    if let Some(child) = &mut child {
        let _ = child.wait();
    }
}
