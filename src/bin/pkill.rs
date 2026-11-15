fn main() {
    std::process::exit(agentcmd::run_with_status("pkill", || {
        agentcmd::commands::pkill::run(&agentcmd::arguments("pkill"))
    }));
}
