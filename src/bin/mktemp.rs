fn main() {
    std::process::exit(agentcmd::run_with_status("mktemp", || {
        agentcmd::commands::mktemp::run(&agentcmd::arguments("mktemp"))
    }));
}
