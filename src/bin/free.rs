fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::free(
        &agentcmd::arguments("free"),
    ));
}
