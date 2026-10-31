fn main() {
    std::process::exit(agentcmd::run_with_status("less", || {
        agentcmd::less_command::run(&agentcmd::arguments("less"))
    }));
}
