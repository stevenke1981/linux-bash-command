fn main() {
    std::process::exit(agentcmd::commands::pathname::run_readlink(
        &agentcmd::arguments("readlink"),
    ));
}
