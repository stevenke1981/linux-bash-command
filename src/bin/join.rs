fn main() {
    std::process::exit(agentcmd::run_with_status("join", || {
        agentcmd::commands::join::run(&agentcmd::arguments("join"))
    }));
}
