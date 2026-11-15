fn main() {
    std::process::exit(agentcmd::run_with_status("rmdir", || {
        agentcmd::commands::fsops::rmdir::run(&agentcmd::arguments("rmdir"))
    }));
}
