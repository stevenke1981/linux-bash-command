fn main() {
    std::process::exit(agentcmd::commands::expand::run_unexpand(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
