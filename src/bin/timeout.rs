fn main() {
    std::process::exit(agentcmd::run_with_status("timeout", || {
        agentcmd::commands::timeout::run(&agentcmd::arguments("timeout"))
    }));
}
