fn main() {
    std::process::exit(agentcmd::run_with_status("cd", || {
        agentcmd::cd_command::run(&agentcmd::arguments("cd"))
    }));
}
