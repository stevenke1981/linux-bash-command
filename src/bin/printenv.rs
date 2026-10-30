fn main() {
    std::process::exit(agentcmd::commands::env::run_printenv(&agentcmd::arguments(
        "printenv",
    )));
}
