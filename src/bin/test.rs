fn main() {
    let args = agentcmd::arguments("test");
    let bracket = std::env::args()
        .next()
        .and_then(|p| std::path::Path::new(&p).file_stem().map(|s| s == "["))
        .unwrap_or(false);
    std::process::exit(if bracket {
        agentcmd::commands::test::run_bracket(&args)
    } else {
        agentcmd::commands::test::run(&args)
    });
}
