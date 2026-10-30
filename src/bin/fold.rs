fn main() {
    std::process::exit(agentcmd::commands::fold::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
