fn main() {
    std::process::exit(agentcmd::run_with_status("rg", || {
        agentcmd::search::run_rg(&agentcmd::arguments("rg"))
    }));
}
