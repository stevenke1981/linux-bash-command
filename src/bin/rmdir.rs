fn main() {
    std::process::exit(agentcmd::commands::fsops::rmdir::run(&agentcmd::arguments(
        "rmdir",
    )));
}
