fn main() {
    std::process::exit(agentcmd::commands::realpath::readlink(
        &agentcmd::arguments("readlink"),
    ));
}
