fn main() {
    std::process::exit(agentcmd::run_with_status("zcat", || {
        agentcmd::commands::zcat::run(&agentcmd::arguments("zcat"))
    }));
}
