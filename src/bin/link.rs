fn main() {
    std::process::exit(agentcmd::commands::fsops::link::run_link(
        &agentcmd::arguments("link"),
    ));
}
