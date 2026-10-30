fn main() {
    std::process::exit(agentcmd::commands::pgrep::pgrep(&agentcmd::arguments(
        "pgrep",
    )));
}
