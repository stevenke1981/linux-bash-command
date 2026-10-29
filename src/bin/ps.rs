fn main() {
    std::process::exit(agentcmd::ps_command::run(&agentcmd::arguments("ps")));
}
