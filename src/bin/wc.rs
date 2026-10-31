fn main() {
    std::process::exit(agentcmd::run_with_status("wc", || {
        agentcmd::commands::wc::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
