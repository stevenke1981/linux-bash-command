fn main() {
    std::process::exit(agentcmd::commands::tac::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
