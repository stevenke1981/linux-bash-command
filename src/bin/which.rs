fn main() {
    std::process::exit(agentcmd::commands::which::run(&agentcmd::arguments(
        "which",
    )));
}
