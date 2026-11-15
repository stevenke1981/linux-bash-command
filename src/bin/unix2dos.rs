fn main() {
    std::process::exit(agentcmd::run_with_status("unix2dos", || {
        agentcmd::commands::unix2dos::run(&agentcmd::arguments("unix2dos"))
    }));
}
