fn main() {
    std::process::exit(agentcmd::run_with_status("uname", || {
        agentcmd::commands::uname::run(&agentcmd::arguments("uname"))
    }));
}
