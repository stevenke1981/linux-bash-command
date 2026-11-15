fn main() {
    std::process::exit(agentcmd::commands::basenc::run_basenc(
        &agentcmd::arguments("basenc"),
    ));
}
