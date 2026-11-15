fn main() {
    std::process::exit(agentcmd::run_with_status("shuf", || {
        agentcmd::commands::shuf::run(&agentcmd::arguments("shuf"))
    }));
}
