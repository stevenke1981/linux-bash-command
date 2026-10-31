fn main() {
    std::process::exit(agentcmd::run_with_status("diff", || {
        agentcmd::diff::run(&agentcmd::arguments("diff"))
    }));
}
