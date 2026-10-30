fn main() {
    std::process::exit(agentcmd::commands::expand::run_unexpand(
        &agentcmd::arguments("unexpand"),
    ));
}
