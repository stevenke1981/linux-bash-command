fn main() {
    std::process::exit(agentcmd::run_with_status("cp", || {
        agentcmd::commands::fsops::cp::run(&agentcmd::arguments("cp"))
    }));
}
