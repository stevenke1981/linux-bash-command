fn main() {
    std::process::exit(agentcmd::commands::patch::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
