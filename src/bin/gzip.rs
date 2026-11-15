fn main() {
    std::process::exit(agentcmd::run_with_status("gzip", || {
        agentcmd::commands::gzip::run(&agentcmd::arguments("gzip"))
    }));
}
