fn main() {
    std::process::exit(agentcmd::commands::chmod::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
