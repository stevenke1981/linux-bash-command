fn main() {
    std::process::exit(agentcmd::run_with_status("df", || {
        agentcmd::df_command::run(&agentcmd::arguments("df"))
    }));
}
