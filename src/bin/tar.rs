fn main() {
    std::process::exit(agentcmd::run_with_status("tar", || {
        agentcmd::commands::tar::run(&agentcmd::arguments("tar"))
    }));
}
