fn main() {
    std::process::exit(agentcmd::commands::pathname::dirname(&agentcmd::arguments(
        "dirname",
    )));
}
