fn main() {
    std::process::exit(agentcmd::run_with_status("tr", || {
        agentcmd::commands::tr::run(&std::env::args().skip(1).collect::<Vec<_>>())
    }));
}
