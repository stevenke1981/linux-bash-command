fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::uptime(
        &agentcmd::arguments("uptime"),
    ));
}
