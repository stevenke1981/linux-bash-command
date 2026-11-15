fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_sha256sum(
        &agentcmd::arguments("sha256sum"),
    ));
}
