fn main() {
    std::process::exit(agentcmd::commands::xargs::run(&agentcmd::arguments(
        "xargs",
    )));
}
