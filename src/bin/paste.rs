fn main() {
    std::process::exit(agentcmd::run_with_status("paste", || {
        agentcmd::commands::paste::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
