fn main() {
    std::process::exit(agentcmd::commands::tee::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
