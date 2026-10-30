fn main() {
    std::process::exit(agentcmd::commands::expand::run_expand(
        &agentcmd::arguments("expand"),
    ));
}
