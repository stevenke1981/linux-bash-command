fn main() {
    std::process::exit(agentcmd::commands::hashsum::md5sum(&agentcmd::arguments(
        "md5sum",
    )));
}
