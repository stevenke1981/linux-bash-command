fn main() {
    std::process::exit(agentcmd::commands::hashsum::sha256sum(
        &agentcmd::arguments("sha256sum"),
    ));
}
