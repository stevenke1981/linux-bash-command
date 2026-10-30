fn main() {
    let args = agentcmd::arguments("test");
    let argv0 = std::env::args_os().next().unwrap_or_default();
    let base = std::path::Path::new(&argv0)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let status = if base == "[" {
        agentcmd::commands::test::run_bracket(&args)
    } else {
        agentcmd::commands::test::run(&args)
    };
    std::process::exit(status);
}
