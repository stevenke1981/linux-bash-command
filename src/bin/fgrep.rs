fn main() {
    std::process::exit(agentcmd::commands::fgrep::run(&agentcmd::arguments(
        "fgrep",
    )));
}
