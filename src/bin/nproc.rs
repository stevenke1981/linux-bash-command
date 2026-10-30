fn main() {
    std::process::exit(agentcmd::commands::sysinfo_cmds::nproc(
        &agentcmd::arguments("nproc"),
    ));
}
