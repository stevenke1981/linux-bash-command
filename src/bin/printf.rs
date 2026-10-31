fn main() {
    std::process::exit(agentcmd::run_with_status("printf", || {
        agentcmd::commands::printf::run(&agentcmd::arguments("printf"))
    }));
}
