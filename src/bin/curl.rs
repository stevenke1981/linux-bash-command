fn main() {
    let args = agentcmd::arguments("curl");
    std::process::exit(agentcmd::run_with_status("curl", || {
        agentcmd::curl_command::run(&args)
    }));
}
