fn main() {
    std::process::exit(agentcmd::commands::csplit::run(&agentcmd::arguments(
        "csplit",
    )));
}
