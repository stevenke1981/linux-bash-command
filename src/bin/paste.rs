fn main() {
    std::process::exit(agentcmd::commands::paste::run(&agentcmd::arguments(
        "paste",
    )));
}
