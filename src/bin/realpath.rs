fn main() {
    std::process::exit(agentcmd::commands::realpath::realpath(
        &agentcmd::arguments("realpath"),
    ));
}
