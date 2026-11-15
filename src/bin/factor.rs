fn main() {
    std::process::exit(agentcmd::run_with_status("factor", || {
        agentcmd::commands::factor::run(&agentcmd::arguments("factor"))
    }));
}
