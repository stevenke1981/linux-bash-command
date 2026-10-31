fn main() {
    std::process::exit(agentcmd::run_with_status("true", || {
        agentcmd::commands::truefalse::run_true(&agentcmd::arguments("true"))
    }));
}
