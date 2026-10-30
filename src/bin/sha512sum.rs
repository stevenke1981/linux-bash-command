fn main() {
    std::process::exit(agentcmd::commands::hashsum::sha512sum(
        &agentcmd::arguments("sha512sum"),
    ));
}
