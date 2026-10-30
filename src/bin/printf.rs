fn main() {
    std::process::exit(agentcmd::commands::printf::run(&agentcmd::arguments(
        "printf",
    )));
}
