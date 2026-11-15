fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_sha224sum(
        &agentcmd::arguments("sha224sum"),
    ));
}
