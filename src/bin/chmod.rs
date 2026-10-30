fn main() {
    std::process::exit(agentcmd::commands::fsops::chmod::run(&agentcmd::arguments(
        "chmod",
    )));
}
