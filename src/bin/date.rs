fn main() {
    std::process::exit(agentcmd::run_with_status("date", || {
        agentcmd::commands::date::run(&agentcmd::arguments("date"))
    }));
}
