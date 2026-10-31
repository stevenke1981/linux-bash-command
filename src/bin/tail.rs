fn main() {
    std::process::exit(agentcmd::run_with_status("tail", || {
        agentcmd::text_commands::tail(&agentcmd::arguments("tail"))
    }));
}
