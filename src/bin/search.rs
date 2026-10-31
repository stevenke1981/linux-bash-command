fn main() {
    std::process::exit(agentcmd::run_with_status("search", || {
        agentcmd::search::run(&agentcmd::arguments("search"))
    }));
}
