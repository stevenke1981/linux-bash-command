fn main() {
    std::process::exit(agentcmd::run_with_status("rm", || {
        agentcmd::commands::fsops::rm::run(&agentcmd::arguments("rm"))
    }));
}
