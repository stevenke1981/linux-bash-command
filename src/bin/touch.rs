fn main() {
    std::process::exit(agentcmd::commands::fsops::touch::run(&agentcmd::arguments(
        "touch",
    )));
}
