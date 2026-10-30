fn main() {
    std::process::exit(agentcmd::commands::fsops::mv::run(&agentcmd::arguments(
        "mv",
    )));
}
