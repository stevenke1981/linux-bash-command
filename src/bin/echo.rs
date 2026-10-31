fn main() {
    std::process::exit(agentcmd::run_with_status("echo", || {
        agentcmd::commands::echo::run(&agentcmd::arguments("echo"))
    }));
}
