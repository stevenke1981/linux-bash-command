fn main() {
    std::process::exit(agentcmd::commands::pgrep::pkill(&agentcmd::arguments(
        "pkill",
    )));
}
