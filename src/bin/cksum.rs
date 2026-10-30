fn main() {
    std::process::exit(agentcmd::commands::cksum::cksum(&agentcmd::arguments(
        "cksum",
    )));
}
