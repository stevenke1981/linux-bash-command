fn main() {
    std::process::exit(agentcmd::commands::pathname::run_dirname(
        &agentcmd::arguments("dirname"),
    ));
}
