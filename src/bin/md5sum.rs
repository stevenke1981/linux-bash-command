fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_md5sum(
        &agentcmd::arguments("md5sum"),
    ));
}
