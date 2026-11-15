fn main() {
    std::process::exit(agentcmd::run_with_status("split", || {
        agentcmd::commands::split::run(&agentcmd::arguments("split"))
    }));
}
