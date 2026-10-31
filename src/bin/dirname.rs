fn main() {
    std::process::exit(agentcmd::run_with_status("dirname", || {
        agentcmd::commands::pathname::run_dirname(&agentcmd::arguments("dirname"))
    }));
}
