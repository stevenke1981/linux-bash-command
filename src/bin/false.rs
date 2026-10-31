fn main() {
    std::process::exit(agentcmd::run_with_status("false", || {
        agentcmd::commands::truefalse::run_false(&agentcmd::arguments("false"))
    }));
}
