fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_b2sum(
        &agentcmd::arguments("b2sum"),
    ));
}
