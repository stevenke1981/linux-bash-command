fn main() {
    std::process::exit(agentcmd::run_with_status("which", || {
        agentcmd::commands::which::run(&agentcmd::arguments("which"))
    }));
}
