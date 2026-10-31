fn main() {
    std::process::exit(agentcmd::run_with_status("egrep", || {
        agentcmd::commands::egrep::run(&agentcmd::arguments("egrep"))
    }));
}
