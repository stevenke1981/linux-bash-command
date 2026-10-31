fn main() {
    std::process::exit(agentcmd::run_with_status("ls", || {
        agentcmd::fs_commands::ls(&agentcmd::arguments("ls"))
    }));
}
