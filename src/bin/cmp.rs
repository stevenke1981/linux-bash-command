fn main() {
    std::process::exit(agentcmd::run_with_status("cmp", || {
        agentcmd::commands::cmp::run(&agentcmd::arguments("cmp"))
    }));
}
