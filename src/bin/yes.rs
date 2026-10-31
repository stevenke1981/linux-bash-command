fn main() {
    std::process::exit(agentcmd::run_with_status("yes", || {
        agentcmd::commands::yes::run(&agentcmd::arguments("yes"))
    }));
}
