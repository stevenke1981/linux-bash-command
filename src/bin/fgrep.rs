fn main() {
    std::process::exit(agentcmd::run_with_status("fgrep", || {
        agentcmd::commands::fgrep::run(&agentcmd::arguments("fgrep"))
    }));
}
