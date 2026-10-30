fn main() {
    std::process::exit(agentcmd::commands::pathname::run_basename(
        &agentcmd::arguments("basename"),
    ));
}
