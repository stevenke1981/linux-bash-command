fn main() {
    std::process::exit(agentcmd::commands::split::run(&agentcmd::arguments(
        "split",
    )));
}
