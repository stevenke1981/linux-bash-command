fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::uname(
        &agentcmd::arguments("uname"),
    ));
}
