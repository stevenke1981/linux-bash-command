fn main() {
    std::process::exit(agentcmd::commands::dos2unix::dos2unix(
        &agentcmd::arguments("dos2unix"),
    ));
}
