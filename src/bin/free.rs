fn main() {
    std::process::exit(agentcmd::run_with_status("free", || {
        agentcmd::commands::free::run(&agentcmd::arguments("free"))
    }));
}
