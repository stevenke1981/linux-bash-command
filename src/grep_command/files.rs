//! File-system helpers for grep: device detection, `--exclude-from` lists, the `-T`
//! field width and error text.

use std::fs;
use std::io;

/// Width used for `-T` line and byte-offset fields when the input size is unknown
/// (pipes). GNU grep 3.11 on Ubuntu 24.04 uses this width there.
const UNKNOWN_SIZE_WIDTH: usize = 18;

/// Reads a `--exclude-from` file: one glob per line, with a final newline optional.
pub(super) fn read_glob_file(path: &str) -> Result<Vec<String>, String> {
    let bytes = fs::read(path).map_err(|error| format!("{path}: {}", io_message(&error)))?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let body = text.strip_suffix('\n').unwrap_or(text.as_str());
    Ok(body.split('\n').map(str::to_string).collect())
}

/// True for block and character devices, FIFOs and sockets.
#[cfg(unix)]
pub(super) fn is_device_type(file_type: fs::FileType) -> bool {
    use std::os::unix::fs::FileTypeExt;
    file_type.is_block_device()
        || file_type.is_char_device()
        || file_type.is_fifo()
        || file_type.is_socket()
}

/// Windows has no device or FIFO file types visible to `std`.
#[cfg(not(unix))]
pub(super) fn is_device_type(_file_type: fs::FileType) -> bool {
    false
}

/// Number of decimal digits in the input size, which sets the `-T` field width.
pub(super) fn offset_width(size: Option<u64>) -> usize {
    match size {
        Some(bytes) => bytes.to_string().len(),
        None => UNKNOWN_SIZE_WIDTH,
    }
}

/// Size of standard input when it is a regular file (`grep -T < file`).
pub(super) fn stdin_size() -> Option<u64> {
    #[cfg(unix)]
    {
        let meta = fs::metadata("/dev/stdin").ok()?;
        meta.is_file().then_some(meta.len())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

pub(super) fn io_message(error: &io::Error) -> String {
    crate::common::error::io_error_text(error)
}
