pub(super) mod codec;
pub(super) mod decode;
mod files;
mod options;
pub fn run_named(command: &'static str, argv: &[String]) -> i32 {
    match options::parse(command, argv) {
        Ok(o) => files::run(&o),
        Err(code) => code,
    }
}
pub fn run(argv: &[String]) -> i32 {
    run_named("gzip", argv)
}
