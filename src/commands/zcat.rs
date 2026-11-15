pub fn run(argv: &[String]) -> i32 {
    super::gzip::run_named("zcat", argv)
}
