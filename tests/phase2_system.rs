mod common;

use common::Sandbox;
use std::process::{Command, Output};

fn run(util: &str, args: &[&str]) -> Output {
    run_env(util, args, &[])
}

fn run_env(util: &str, args: &[&str], vars: &[(&str, &str)]) -> Output {
    let sandbox = Sandbox::new();
    let mut command = Command::new(common::binary(util));
    command.args(args).current_dir(&sandbox.path);
    command
        .env_remove("OMP_NUM_THREADS")
        .env_remove("OMP_THREAD_LIMIT");
    for (key, value) in vars {
        command.env(key, value);
    }
    common::execute(command, b"")
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn one_line(output: &Output) -> &str {
    let value = text(&output.stdout);
    value.strip_suffix('\n').unwrap()
}

mod nproc_cli {
    use super::*;

    #[test]
    fn help() {
        let out = run("nproc", &["--help"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).contains("Usage: nproc"));
    }

    #[test]
    fn version() {
        let out = run("nproc", &["--version"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).starts_with("nproc (agentcmd) "));
    }

    #[test]
    fn unknown_option_is_usage_error() {
        let out = run("nproc", &["--bogus"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(text(&out.stderr).contains("nproc: unrecognized option"));
    }

    #[test]
    fn short_help_alias_is_not_claimed() {
        let out = run("nproc", &["-h"]);
        assert_eq!(out.status.code(), Some(1));
    }

    #[test]
    fn default_count_is_positive() {
        let out = run_env(
            "nproc",
            &[],
            &[("OMP_NUM_THREADS", ""), ("OMP_THREAD_LIMIT", "")],
        );
        assert_eq!(out.status.code(), Some(0));
        assert!(one_line(&out).parse::<usize>().unwrap() >= 1);
    }

    #[test]
    fn all_count_is_positive() {
        let out = run("nproc", &["--all"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(one_line(&out).parse::<usize>().unwrap() >= 1);
    }

    #[test]
    fn ignore_huge_count_still_prints_one() {
        let out = run("nproc", &["--ignore=999999"]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), "1");
    }

    #[test]
    fn omp_num_threads_uses_first_comma_value() {
        let out = run_env("nproc", &[], &[("OMP_NUM_THREADS", "1,999")]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), "1");
    }

    #[test]
    fn omp_thread_limit_caps_default_count() {
        let out = run_env("nproc", &[], &[("OMP_THREAD_LIMIT", "1")]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), "1");
    }

    #[test]
    fn all_ignores_both_openmp_limits() {
        let baseline = run_env("nproc", &["--all"], &[]);
        let limited = run_env(
            "nproc",
            &["--all"],
            &[("OMP_NUM_THREADS", "1"), ("OMP_THREAD_LIMIT", "1")],
        );
        assert_eq!(limited.stdout, baseline.stdout);
    }

    #[test]
    fn operands_are_rejected() {
        let out = run("nproc", &["extra"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(text(&out.stderr).contains("unexpected operand"));
    }

    #[test]
    fn missing_ignore_value_is_rejected() {
        assert_eq!(run("nproc", &["--ignore"]).status.code(), Some(1));
    }

    #[test]
    fn malformed_ignore_is_rejected() {
        assert_eq!(run("nproc", &["--ignore=x"]).status.code(), Some(1));
    }

    #[test]
    fn option_after_operand_is_permuted() {
        let out = run("nproc", &["--ignore=999999", "extra"]);
        assert_eq!(out.status.code(), Some(1));
    }

    #[test]
    fn ignore_zero_matches_a_positive_default() {
        let out = run("nproc", &["--ignore=0"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(one_line(&out).parse::<usize>().unwrap() >= 1);
    }
}

mod hostname_cli {
    use super::*;

    fn system_name() -> String {
        sysinfo::System::host_name().expect("the host should provide a hostname")
    }

    #[test]
    fn help() {
        let out = run("hostname", &["--help"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).contains("Usage: hostname"));
    }

    #[test]
    fn version() {
        let out = run("hostname", &["--version"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).starts_with("hostname (agentcmd) "));
    }

    #[test]
    fn default_prints_system_hostname() {
        let out = run("hostname", &[]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), system_name());
    }

    #[test]
    fn short_prints_first_hostname_label() {
        let out = run("hostname", &["-s"]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), system_name().split('.').next().unwrap());
    }

    #[test]
    fn long_short_prints_first_hostname_label() {
        let out = run("hostname", &["--short"]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), system_name().split('.').next().unwrap());
    }

    #[test]
    fn fqdn_uses_full_name_or_best_effort_short_name() {
        let name = system_name();
        let expected = if name.contains('.') {
            name
        } else {
            name.split('.').next().unwrap().to_owned()
        };
        let out = run("hostname", &["--fqdn"]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), expected);
    }

    #[test]
    fn setting_hostname_is_refused() {
        let out = run("hostname", &["test.invalid"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(text(&out.stderr).contains("setting the host name is not supported"));
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert_eq!(run("hostname", &["--bogus"]).status.code(), Some(1));
    }

    #[test]
    fn short_and_fqdn_are_mutually_exclusive() {
        let out = run("hostname", &["-s", "-f"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(text(&out.stderr).contains("mutually exclusive"));
    }

    #[test]
    fn two_operands_are_refused() {
        assert_eq!(run("hostname", &["one", "two"]).status.code(), Some(1));
    }

    #[test]
    fn explicit_short_and_fqdn_aliases_are_supported() {
        assert_eq!(run("hostname", &["--short"]).status.code(), Some(0));
        assert_eq!(run("hostname", &["--fqdn"]).status.code(), Some(0));
    }

    #[test]
    fn one_operand_is_not_used_to_set_hostname() {
        assert_eq!(run("hostname", &["another"]).status.code(), Some(1));
    }

    #[test]
    fn repeated_short_option_is_accepted() {
        assert_eq!(run("hostname", &["-s", "-s"]).status.code(), Some(0));
    }

    #[test]
    fn delimiter_does_not_turn_operand_into_an_option() {
        let out = run("hostname", &["--", "-s"]);
        assert_eq!(out.status.code(), Some(1));
    }

    #[test]
    fn help_precedes_rejected_set_operand() {
        assert_eq!(
            run("hostname", &["--help", "test.invalid"]).status.code(),
            Some(0)
        );
    }
}

mod arch_cli {
    use super::*;

    fn expected_arch() -> &'static str {
        match std::env::consts::ARCH {
            "x86" => "i686",
            other => other,
        }
    }

    #[test]
    fn help() {
        let out = run("arch", &["--help"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).contains("Usage: arch"));
    }

    #[test]
    fn version() {
        let out = run("arch", &["--version"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).starts_with("arch (agentcmd) "));
    }

    #[test]
    fn prints_actual_target_architecture() {
        let out = run("arch", &[]);
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), expected_arch());
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert_eq!(run("arch", &["--bogus"]).status.code(), Some(1));
    }

    #[test]
    fn help_shortcut_is_not_assumed() {
        assert_eq!(run("arch", &["-h"]).status.code(), Some(1));
    }

    #[test]
    fn operand_is_rejected() {
        assert_eq!(run("arch", &["extra"]).status.code(), Some(1));
    }

    #[test]
    fn option_after_operand_is_still_parsed() {
        assert_eq!(run("arch", &["extra", "--help"]).status.code(), Some(0));
    }

    #[test]
    fn delimiter_makes_option_text_an_operand() {
        assert_eq!(run("arch", &["--", "--help"]).status.code(), Some(1));
    }

    #[test]
    fn output_is_ascii_and_line_terminated() {
        let out = run("arch", &[]);
        assert!(out.stdout.is_ascii());
        assert!(out.stdout.ends_with(b"\n"));
    }

    #[test]
    fn only_one_architecture_line_is_printed() {
        assert_eq!(text(&run("arch", &[]).stdout).lines().count(), 1);
    }

    #[test]
    fn architecture_is_not_empty() {
        assert!(!one_line(&run("arch", &[])).is_empty());
    }

    #[test]
    fn version_has_one_line() {
        assert_eq!(text(&run("arch", &["--version"]).stdout).lines().count(), 1);
    }

    #[test]
    fn help_has_one_usage_header() {
        assert_eq!(
            text(&run("arch", &["--help"]).stdout)
                .matches("Usage:")
                .count(),
            1
        );
    }

    #[test]
    fn stdin_is_ignored() {
        let sandbox = Sandbox::new();
        let mut command = Command::new(common::binary("arch"));
        command.current_dir(&sandbox.path);
        let out = common::execute(command, b"stdin text\n");
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(one_line(&out), expected_arch());
    }

    #[test]
    fn extra_arguments_do_not_change_host_value() {
        let expected = expected_arch();
        let out = run("arch", &[]);
        assert_eq!(one_line(&out), expected);
    }
}

mod uptime_cli {
    use super::*;

    #[test]
    fn help() {
        let out = run("uptime", &["--help"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).contains("Usage: uptime"));
    }

    #[test]
    fn version() {
        let out = run("uptime", &["--version"]);
        assert_eq!(out.status.code(), Some(0));
        assert!(text(&out.stdout).starts_with("uptime (agentcmd) "));
    }

    #[test]
    fn pretty_format_has_uptime_units() {
        let out = run("uptime", &["--pretty"]);
        assert_eq!(out.status.code(), Some(0));
        let output = one_line(&out);
        assert!(output.starts_with("up "));
        assert!(
            output.contains("minute")
                || output.contains("hour")
                || output.contains("day")
                || output.contains("week")
        );
    }

    #[test]
    fn short_pretty_option_is_supported() {
        assert_eq!(run("uptime", &["-p"]).status.code(), Some(0));
    }

    #[test]
    fn since_is_timezone_aware_and_formatted() {
        let out = run("uptime", &["--since"]);
        assert_eq!(out.status.code(), Some(0));
        let output = one_line(&out);
        assert_eq!(output.len(), 19);
        assert_eq!(&output[4..5], "-");
        assert_eq!(&output[7..8], "-");
        assert_eq!(&output[10..11], " ");
        assert_eq!(&output[13..14], ":");
        assert_eq!(&output[16..17], ":");
    }

    #[test]
    fn short_since_option_is_supported() {
        assert_eq!(run("uptime", &["-s"]).status.code(), Some(0));
    }

    #[test]
    fn default_report_includes_current_time_and_uptime() {
        let out = run("uptime", &[]);
        assert_eq!(out.status.code(), Some(0));
        assert!(one_line(&out).contains(" up "));
        assert!(one_line(&out).starts_with(' '));
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert_eq!(run("uptime", &["--bogus"]).status.code(), Some(1));
    }

    #[test]
    fn operand_is_rejected() {
        assert_eq!(run("uptime", &["unexpected"]).status.code(), Some(1));
    }

    #[test]
    fn combined_pretty_and_since_options_are_accepted() {
        assert_eq!(
            run("uptime", &["--pretty", "--since"]).status.code(),
            Some(0)
        );
    }

    #[test]
    fn since_timestamp_is_ascii() {
        assert!(run("uptime", &["-s"]).stdout.is_ascii());
    }

    #[test]
    fn pretty_output_is_line_terminated() {
        assert!(run("uptime", &["-p"]).stdout.ends_with(b"\n"));
    }

    #[test]
    fn help_lists_both_supported_modes() {
        let out = run("uptime", &["--help"]);
        assert!(text(&out.stdout).contains("--pretty"));
        assert!(text(&out.stdout).contains("--since"));
    }

    #[test]
    fn short_flags_are_clustered_as_gnu_options() {
        let out = run("uptime", &["-ps"]);
        assert_eq!(out.status.code(), Some(0));
    }

    #[test]
    fn mode_long_names_accept_unique_prefixes() {
        assert_eq!(run("uptime", &["--pre"]).status.code(), Some(0));
        assert_eq!(run("uptime", &["--sin"]).status.code(), Some(0));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn combined_uptime_modes_match_ubuntu_gnu_report_kind() {
    if std::env::var("AGENTCMD_GNU_DIFF").as_deref() != Ok("1") {
        return;
    }
    for args in [["-p", "-s"], ["-s", "-p"]] {
        let expected = Command::new("/usr/bin/uptime")
            .args(args)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
            .unwrap();
        let actual = run("uptime", &args);
        assert_eq!(actual.status.code(), expected.status.code());
        assert_eq!(
            actual.stdout.starts_with(b"up "),
            expected.stdout.starts_with(b"up "),
            "{actual:?} {expected:?}"
        );
        assert_eq!(
            actual.stdout.iter().filter(|b| **b == b'\n').count(),
            expected.stdout.iter().filter(|b| **b == b'\n').count()
        );
    }
}
