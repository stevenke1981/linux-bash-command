fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_sha384sum(
        &agentcmd::arguments("sha384sum"),
    ));
}
