fn main() {
    std::process::exit(agentcmd::run_with_status("tac", || {
        agentcmd::commands::tac::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
