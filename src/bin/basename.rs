fn main() {
    std::process::exit(agentcmd::commands::pathname::basename(
        &agentcmd::arguments("basename"),
    ));
}
