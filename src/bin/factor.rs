fn main() {
    std::process::exit(agentcmd::commands::factor::run(&agentcmd::arguments(
        "factor",
    )));
}
