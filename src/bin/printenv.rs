fn main() {
    std::process::exit(agentcmd::run_with_status("printenv", || {
        agentcmd::commands::printenv::run(&agentcmd::arguments("printenv"))
    }));
}
