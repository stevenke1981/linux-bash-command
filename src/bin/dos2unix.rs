fn main() {
    std::process::exit(agentcmd::run_with_status("dos2unix", || {
        agentcmd::commands::dos2unix::run(&agentcmd::arguments("dos2unix"))
    }));
}
