fn main() {
    std::process::exit(agentcmd::commands::tree::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
