fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::whoami(
        &agentcmd::arguments("whoami"),
    ));
}
