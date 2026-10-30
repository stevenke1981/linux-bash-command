fn main() {
    std::process::exit(agentcmd::commands::pathchk::run(&agentcmd::arguments(
        "pathchk",
    )));
}
