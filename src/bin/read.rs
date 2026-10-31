fn main() {
    std::process::exit(agentcmd::run_with_status("read", || {
        agentcmd::files::read(&agentcmd::arguments("read"))
    }));
}
