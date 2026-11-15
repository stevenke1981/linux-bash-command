fn main() {
    std::process::exit(agentcmd::run_with_status("gunzip", || {
        agentcmd::commands::gunzip::run(&agentcmd::arguments("gunzip"))
    }));
}
