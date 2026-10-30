fn main() {
    std::process::exit(agentcmd::commands::tsort::run(&agentcmd::arguments(
        "tsort",
    )));
}
