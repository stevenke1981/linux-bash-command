fn main() {
    std::process::exit(agentcmd::commands::hashsum::b2sum(&agentcmd::arguments(
        "b2sum",
    )));
}
