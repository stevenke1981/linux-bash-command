fn main() {
    std::process::exit(agentcmd::run_with_status("cut", || {
        agentcmd::commands::cut::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
