fn main() {
    std::process::exit(agentcmd::commands::dos2unix::unix2dos(
        &agentcmd::arguments("unix2dos"),
    ));
}
