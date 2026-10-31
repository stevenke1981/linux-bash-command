fn main() {
    std::process::exit(agentcmd::run_with_status("seq", || {
        agentcmd::commands::seq::run(&agentcmd::arguments("seq"))
    }));
}
