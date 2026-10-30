fn main() {
    std::process::exit(agentcmd::commands::truefalse::run_false(
        &agentcmd::arguments("false"),
    ));
}
