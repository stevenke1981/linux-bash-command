fn main() {
    std::process::exit(agentcmd::commands::truncate::run(&agentcmd::arguments(
        "truncate",
    )));
}
