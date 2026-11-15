fn main() {
    std::process::exit(agentcmd::run_with_status("awk", || {
        agentcmd::commands::awk::run(&agentcmd::arguments("awk"))
    }));
}
