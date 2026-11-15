fn main() {
    std::process::exit(agentcmd::run_with_status("csplit", || {
        agentcmd::commands::csplit::run(&agentcmd::arguments("csplit"))
    }));
}
