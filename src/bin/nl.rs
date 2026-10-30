fn main() {
    std::process::exit(agentcmd::commands::nl::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
