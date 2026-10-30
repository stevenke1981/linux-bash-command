fn main() {
    std::process::exit(agentcmd::commands::basenc::base32(&agentcmd::arguments(
        "base32",
    )));
}
