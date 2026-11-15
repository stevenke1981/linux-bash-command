fn main() {
    std::process::exit(agentcmd::run_with_status("unlink", || {
        agentcmd::commands::fsops::unlink::run(&agentcmd::arguments("unlink"))
    }));
}
