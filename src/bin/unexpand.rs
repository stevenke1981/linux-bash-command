fn main() {
    std::process::exit(agentcmd::run_with_status("unexpand", || {
        agentcmd::commands::expand::run_unexpand(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
