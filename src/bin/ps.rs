fn main() {
    std::process::exit(agentcmd::run_with_status("ps", || {
        agentcmd::ps_command::run(&agentcmd::arguments("ps"))
    }));
}
