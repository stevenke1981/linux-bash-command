fn main() {
    std::process::exit(agentcmd::run_with_status("tsort", || {
        agentcmd::commands::tsort::run(&agentcmd::arguments("tsort"))
    }));
}
