fn main() {
    std::process::exit(agentcmd::run_with_status("nproc", || {
        agentcmd::commands::sysinfo_cmds::run_nproc(&agentcmd::arguments("nproc"))
    }));
}
