fn main() {
    std::process::exit(agentcmd::run_with_status("list", || {
        agentcmd::files::list(&agentcmd::arguments("list"))
    }));
}
