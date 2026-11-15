fn main() {
    std::process::exit(agentcmd::run_with_status("arch", || {
        agentcmd::commands::sysinfo_cmds::run_arch(&agentcmd::arguments("arch"))
    }));
}
