fn main() {
    std::process::exit(agentcmd::run_with_status("mv", || {
        agentcmd::commands::fsops::mv::run(&agentcmd::arguments("mv"))
    }));
}
