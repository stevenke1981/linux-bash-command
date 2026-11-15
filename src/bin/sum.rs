fn main() {
    std::process::exit(agentcmd::commands::cksum::run_sum(&agentcmd::arguments(
        "sum",
    )));
}
