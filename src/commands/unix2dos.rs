pub fn run(argv: &[String]) -> i32 {
    super::dos2unix::run_direction(argv, true)
}
