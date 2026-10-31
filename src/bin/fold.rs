fn main() {
    std::process::exit(agentcmd::run_with_status("fold", || {
        agentcmd::commands::fold::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
