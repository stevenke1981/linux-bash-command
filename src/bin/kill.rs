fn main() {
    std::process::exit(agentcmd::commands::signal::run(&agentcmd::arguments(
        "kill",
    )));
}
