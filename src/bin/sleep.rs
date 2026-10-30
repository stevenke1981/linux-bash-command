fn main() {
    std::process::exit(agentcmd::commands::sleep::run(&agentcmd::arguments(
        "sleep",
    )));
}
