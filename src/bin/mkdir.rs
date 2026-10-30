fn main() {
    std::process::exit(agentcmd::commands::fsops::mkdir::run(&agentcmd::arguments(
        "mkdir",
    )));
}
