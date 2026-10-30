fn main() {
    std::process::exit(agentcmd::commands::hashsum::sha384sum(
        &agentcmd::arguments("sha384sum"),
    ));
}
