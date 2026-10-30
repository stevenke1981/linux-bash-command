fn main() {
    std::process::exit(agentcmd::commands::truefalse::run_true(
        &agentcmd::arguments("true"),
    ));
}
