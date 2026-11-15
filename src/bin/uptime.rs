fn main() {
    std::process::exit(agentcmd::run_with_status("uptime", || {
        agentcmd::commands::sysinfo_cmds::run_uptime(&agentcmd::arguments("uptime"))
    }));
}
