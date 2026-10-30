fn main() {
    std::process::exit(agentcmd::commands::patch::run(&agentcmd::arguments(
        "patch",
    )));
}
