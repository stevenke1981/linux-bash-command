fn main() {
    std::process::exit(agentcmd::run_with_status("find", || {
        agentcmd::find_command::find(&agentcmd::arguments("find"))
    }));
}
