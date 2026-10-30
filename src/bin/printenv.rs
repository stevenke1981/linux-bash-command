fn main() {
    std::process::exit(agentcmd::commands::printenv::run(&agentcmd::arguments(
        "printenv",
    )));
}
