fn main() {
    std::process::exit(agentcmd::run_with_status("xargs", || {
        agentcmd::commands::xargs::run(&agentcmd::arguments("xargs"))
    }));
}
