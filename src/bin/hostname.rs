fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::hostname(
        &agentcmd::arguments("hostname"),
    ));
}
