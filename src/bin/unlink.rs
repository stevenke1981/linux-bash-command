fn main() {
    std::process::exit(agentcmd::commands::fsops::link::run_unlink(
        &agentcmd::arguments("unlink"),
    ));
}
