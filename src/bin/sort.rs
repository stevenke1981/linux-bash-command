fn main() {
    std::process::exit(agentcmd::run_with_status("sort", || {
        agentcmd::commands::sort::run(&agentcmd::arguments("sort"))
    }));
}
