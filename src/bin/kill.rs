fn main() {
    std::process::exit(agentcmd::run_with_status("kill", || {
        agentcmd::commands::kill::run(&agentcmd::arguments("kill"))
    }));
}
