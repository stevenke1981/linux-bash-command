fn main() {
    std::process::exit(agentcmd::commands::wc::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
