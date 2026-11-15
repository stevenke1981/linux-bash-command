fn main() {
    std::process::exit(agentcmd::run_with_status("link", || {
        agentcmd::commands::fsops::link::run(&agentcmd::arguments("link"))
    }));
}
