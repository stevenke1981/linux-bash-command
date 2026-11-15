fn main() {
    std::process::exit(agentcmd::run_with_status("sed", || {
        agentcmd::commands::sed::run(&agentcmd::arguments("sed"))
    }));
}
