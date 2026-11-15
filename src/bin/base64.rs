fn main() {
    std::process::exit(agentcmd::commands::basenc::run_base64(
        &agentcmd::arguments("base64"),
    ));
}
