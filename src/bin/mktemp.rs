fn main() {
    std::process::exit(agentcmd::commands::mktemp::run(&agentcmd::arguments(
        "mktemp",
    )));
}
