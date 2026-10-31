fn main() {
    std::process::exit(agentcmd::run_with_status("tee", || {
        agentcmd::commands::tee::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
