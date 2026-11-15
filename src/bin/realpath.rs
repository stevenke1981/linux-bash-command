fn main() {
    std::process::exit(agentcmd::commands::pathname::run_realpath(
        &agentcmd::arguments("realpath"),
    ));
}
