fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::arch(
        &agentcmd::arguments("arch"),
    ));
}
