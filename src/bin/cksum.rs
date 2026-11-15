fn main() {
    std::process::exit(agentcmd::commands::cksum::run(&agentcmd::arguments(
        "cksum",
    )));
}
