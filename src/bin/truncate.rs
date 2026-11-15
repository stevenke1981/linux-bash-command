fn main() {
    std::process::exit(agentcmd::run_with_status("truncate", || {
        agentcmd::commands::truncate::run(&agentcmd::arguments("truncate"))
    }));
}
