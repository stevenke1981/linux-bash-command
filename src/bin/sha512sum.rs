fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_sha512sum(
        &agentcmd::arguments("sha512sum"),
    ));
}
