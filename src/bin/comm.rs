fn main() {
    std::process::exit(agentcmd::run_with_status("comm", || {
        agentcmd::commands::comm::run(&agentcmd::arguments("comm"))
    }));
}
