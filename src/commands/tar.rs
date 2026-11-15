mod create;
mod extract;
mod format;
mod options;
mod path;
mod select;
use options::Mode;
pub fn run(argv: &[String]) -> i32 {
    let o = match options::parse(argv) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let selections = match select::resolve(&o) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tar: {}", crate::common::error::io_text(&e));
            return 2;
        }
    };
    let result = if o.mode == Mode::Create {
        create::run(&o, &selections).map(|()| false)
    } else {
        extract::run(&o, &selections)
    };
    match result {
        Ok(false) => 0,
        Ok(true) => 2,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("tar: {}", crate::common::error::io_text(&e));
            2
        }
    }
}
