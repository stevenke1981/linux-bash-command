fn main() {
    std::process::exit(agentcmd::commands::cut::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
