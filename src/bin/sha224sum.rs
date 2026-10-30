fn main() {
    std::process::exit(agentcmd::commands::hashsum::sha224sum(
        &agentcmd::arguments("sha224sum"),
    ));
}
