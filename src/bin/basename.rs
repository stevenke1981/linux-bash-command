fn main() {
    std::process::exit(agentcmd::run_with_status("basename", || {
        agentcmd::commands::pathname::run_basename(&agentcmd::arguments("basename"))
    }));
}
