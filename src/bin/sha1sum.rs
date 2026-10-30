fn main() {
    std::process::exit(agentcmd::commands::hashsum::sha1sum(&agentcmd::arguments(
        "sha1sum",
    )));
}
