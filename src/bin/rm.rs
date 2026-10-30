fn main() {
    std::process::exit(agentcmd::commands::fsops::rm::run(&agentcmd::arguments(
        "rm",
    )));
}
