fn main() {
    std::process::exit(agentcmd::run_with_status("head", || {
        agentcmd::text_commands::head(&agentcmd::arguments("head"))
    }));
}
