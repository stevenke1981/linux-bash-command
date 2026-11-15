fn main() {
    std::process::exit(agentcmd::commands::column::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ));
}
