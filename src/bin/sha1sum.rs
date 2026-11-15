fn main() {
    std::process::exit(agentcmd::commands::hashsum::run_sha1sum(
        &agentcmd::arguments("sha1sum"),
    ));
}
