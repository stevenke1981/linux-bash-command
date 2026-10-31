fn main() {
    std::process::exit(agentcmd::run_with_status("uniq", || {
        agentcmd::commands::uniq::run(&agentcmd::arguments("uniq"))
    }));
}
