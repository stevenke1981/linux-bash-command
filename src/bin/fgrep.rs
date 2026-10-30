fn main() {
    std::process::exit(agentcmd::commands::grep_alias::fgrep(&agentcmd::arguments(
        "fgrep",
    )));
}
