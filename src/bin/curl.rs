fn main() {
    let args = agentcmd::arguments("curl");
    std::process::exit(agentcmd::curl_command::run(&args));
}
