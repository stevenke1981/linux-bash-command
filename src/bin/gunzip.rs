fn main() {
    std::process::exit(agentcmd::commands::gzip::gunzip(&agentcmd::arguments(
        "gunzip",
    )));
}
