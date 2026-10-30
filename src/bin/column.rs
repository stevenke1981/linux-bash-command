fn main() {
    std::process::exit(agentcmd::commands::column::run(&agentcmd::arguments(
        "column",
    )));
}
