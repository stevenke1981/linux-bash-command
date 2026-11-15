fn main() {
    std::process::exit(agentcmd::run_with_status("whoami", || {
        agentcmd::commands::whoami::run(&agentcmd::arguments("whoami"))
    }));
}
