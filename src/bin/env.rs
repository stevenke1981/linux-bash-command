fn main() {
    std::process::exit(agentcmd::commands::env::run_env(&agentcmd::arguments(
        "env",
    )));
}
