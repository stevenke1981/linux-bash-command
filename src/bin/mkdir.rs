fn main() {
    std::process::exit(agentcmd::run_with_status("mkdir", || {
        agentcmd::commands::fsops::mkdir::run(&agentcmd::arguments("mkdir"))
    }));
}
