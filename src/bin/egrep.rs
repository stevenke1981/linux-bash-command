fn main() {
    std::process::exit(agentcmd::commands::egrep::run(&agentcmd::arguments(
        "egrep",
    )));
}
