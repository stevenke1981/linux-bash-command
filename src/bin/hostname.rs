fn main() {
    std::process::exit(agentcmd::run_with_status("hostname", || {
        agentcmd::commands::sysinfo_cmds::run_hostname(&agentcmd::arguments("hostname"))
    }));
}
