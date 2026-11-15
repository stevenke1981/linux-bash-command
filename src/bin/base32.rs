fn main() {
    std::process::exit(agentcmd::commands::basenc::run_base32(
        &agentcmd::arguments("base32"),
    ));
}
