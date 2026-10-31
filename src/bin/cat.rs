fn main() {
    std::process::exit(agentcmd::run_with_status("cat", || {
        agentcmd::text_commands::cat(&agentcmd::arguments("cat"))
    }));
}
