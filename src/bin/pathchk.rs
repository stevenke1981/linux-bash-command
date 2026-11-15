fn main() {
    std::process::exit(agentcmd::run_with_status("pathchk", || {
        agentcmd::commands::pathchk::run(&agentcmd::arguments("pathchk"))
    }));
}
