fn main() {
    std::process::exit(agentcmd::run_with_status("env", || {
        agentcmd::commands::env::run(&agentcmd::arguments("env"))
    }));
}
