fn main() {
    std::process::exit(agentcmd::run_with_status("pwd", || {
        agentcmd::fs_commands::pwd(&agentcmd::arguments("pwd"))
    }));
}
