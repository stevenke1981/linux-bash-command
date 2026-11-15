fn main() {
    std::process::exit(agentcmd::run_with_status("ln", || {
        agentcmd::commands::fsops::ln::run(&agentcmd::arguments("ln"))
    }));
}
