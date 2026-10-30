fn main() {
    std::process::exit(agentcmd::commands::fsops::ln::run(&agentcmd::arguments(
        "ln",
    )));
}
