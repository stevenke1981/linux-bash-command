fn main() {
    std::process::exit(agentcmd::run_with_status("rev", || {
        agentcmd::commands::rev::run(&agentcmd::arguments("rev"))
    }));
}
