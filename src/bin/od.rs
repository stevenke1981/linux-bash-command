fn main() {
    std::process::exit(agentcmd::run_with_status("od", || {
        agentcmd::commands::od::run(&agentcmd::arguments("od"))
    }));
}
