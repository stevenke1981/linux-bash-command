fn main() {
    std::process::exit(agentcmd::commands::fsops::cp::run(&agentcmd::arguments(
        "cp",
    )));
}
