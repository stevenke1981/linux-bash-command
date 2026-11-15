fn main() {
    std::process::exit(agentcmd::run_with_status("pgrep", || {
        agentcmd::commands::pgrep::run_pgrep(&agentcmd::arguments("pgrep"))
    }));
}
