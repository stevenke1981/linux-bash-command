fn main() {
    std::process::exit(agentcmd::commands::grep_alias::egrep(&agentcmd::arguments(
        "egrep",
    )));
}
