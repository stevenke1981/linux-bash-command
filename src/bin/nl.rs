fn main() {
    std::process::exit(agentcmd::run_with_status("nl", || {
        agentcmd::commands::nl::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
