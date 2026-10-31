fn main() {
    std::process::exit(agentcmd::run_with_status("grep", || {
        agentcmd::grep_command::run(&agentcmd::arguments("grep"))
    }));
}
