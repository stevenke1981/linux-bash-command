fn main() {
    std::process::exit(agentcmd::run_with_status("sleep", || {
        agentcmd::commands::sleep::run(&agentcmd::arguments("sleep"))
    }));
}
