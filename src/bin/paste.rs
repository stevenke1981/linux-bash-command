fn main() {
    std::process::exit(agentcmd::commands::paste::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
