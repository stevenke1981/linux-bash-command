fn main() {
    std::process::exit(agentcmd::commands::timeout::run(&agentcmd::arguments(
        "timeout",
    )));
}
