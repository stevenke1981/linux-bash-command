fn main() {
    std::process::exit(agentcmd::run_with_status("touch", || {
        agentcmd::commands::fsops::touch::run(&agentcmd::arguments("touch"))
    }));
}
