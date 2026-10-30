fn main() {
    std::process::exit(agentcmd::commands::numfmt::run(&agentcmd::arguments(
        "numfmt",
    )));
}
