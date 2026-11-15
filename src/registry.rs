pub type RunFn = fn(&[String]) -> i32;

pub struct Utility {
    pub name: &'static str,
    pub run: RunFn,
    pub summary: &'static str,
    pub origin: Origin,
    pub windows_conflict: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Existing,
    Ms,
    Extra,
}

macro_rules! utilities {
    ($($name:literal => $run:path, $summary:literal, $origin:ident, $conflict:expr;)*) => {
        pub static UTILITIES: &[Utility] = &[$(Utility {
            name: $name, run: $run, summary: $summary,
            origin: Origin::$origin, windows_conflict: $conflict,
        }),*];
    };
}

utilities! {
    "[" => crate::commands::test::run_bracket, "Evaluate a bracketed expression", Ms, None;
    "arch" => crate::commands::sysinfo_cmds::run_arch, "Report arch system information", Ms, None;
    "b2sum" => crate::commands::hashsum::run_b2sum, "Compute b2sum checksums", Ms, None;
    "base32" => crate::commands::basenc::run_base32, "Encode and decode base32 streams", Ms, None;
    "base64" => crate::commands::basenc::run_base64, "Encode and decode base64 streams", Ms, None;
    "basename" => crate::commands::pathname::run_basename, "Remove directory prefix", Ms, None;
    "basenc" => crate::commands::basenc::run_basenc, "Encode and decode basenc streams", Ms, None;
    "cat" => crate::text_commands::cat, "Concatenate files", Existing, Some("PowerShell");
    "cd" => crate::cd_command::run, "Resolve a directory", Existing, Some("PowerShell");
    "cksum" => crate::commands::cksum::run, "Compute cksum checksums", Ms, None;
    "comm" => crate::commands::comm::run, "Compare sorted files", Ms, None;
    "cp" => crate::commands::fsops::cp::run, "Cp filesystem entries", Ms, None;
    "curl" => crate::curl_command::run, "Transfer data from URLs", Existing, Some("PowerShell 5.1");
    "cut" => crate::commands::cut::run, "Select fields or bytes", Ms, None;
    "date" => crate::commands::date::run, "Display or format dates", Ms, None;
    "df" => crate::df_command::run, "Report filesystem space", Existing, None;
    "diff" => crate::diff::run, "Compare files", Existing, Some("PowerShell 5.1");
    "dirname" => crate::commands::pathname::run_dirname, "Remove final path component", Ms, None;
    "du" => crate::commands::du::run, "Report disk usage", Ms, None;
    "echo" => crate::commands::echo::run, "Display a line of text", Ms, Some("PowerShell");
    "egrep" => crate::commands::egrep::run, "Search extended regular expressions", Extra, None;
    "env" => crate::commands::env::run, "Run a command in a modified environment", Ms, None;
    "expand" => crate::commands::expand::run_expand, "Convert expand to spaces", Ms, Some("Windows System32");
    "false" => crate::commands::truefalse::run_false, "Exit unsuccessfully", Ms, None;
    "fgrep" => crate::commands::fgrep::run, "Search fixed strings", Extra, None;
    "find" => crate::find_command::find, "Find files", Existing, Some("Windows");
    "fold" => crate::commands::fold::run, "Wrap input lines", Ms, None;
    "grep" => crate::grep_command::run, "Search text", Existing, None;
    "head" => crate::text_commands::head, "Print first lines", Existing, None;
    "hostname" => crate::commands::sysinfo_cmds::run_hostname, "Report hostname system information", Ms, Some("Windows System32");
    "less" => crate::less_command::run, "Display files", Existing, None;
    "link" => crate::commands::fsops::link::run, "Link filesystem entries", Ms, Some("MSVC linker");
    "list" => crate::files::list, "List files recursively", Existing, None;
    "ln" => crate::commands::fsops::ln::run, "Ln filesystem entries", Ms, None;
    "ls" => crate::fs_commands::ls, "List directory contents", Existing, Some("PowerShell");
    "md5sum" => crate::commands::hashsum::run_md5sum, "Compute md5sum checksums", Ms, None;
    "mkdir" => crate::commands::fsops::mkdir::run, "Mkdir filesystem entries", Ms, None;
    "mktemp" => crate::commands::mktemp::run, "Create a temporary file or directory", Ms, None;
    "mv" => crate::commands::fsops::mv::run, "Mv filesystem entries", Ms, None;
    "nl" => crate::commands::nl::run, "Number input lines", Ms, None;
    "nproc" => crate::commands::sysinfo_cmds::run_nproc, "Report nproc system information", Ms, None;
    "paste" => crate::commands::paste::run, "Merge lines", Ms, None;
    "printenv" => crate::commands::printenv::run, "Print environment values", Ms, None;
    "printf" => crate::commands::printf::run, "Format arguments", Ms, None;
    "ps" => crate::ps_command::run, "List processes", Existing, Some("PowerShell");
    "pwd" => crate::fs_commands::pwd, "Print working directory", Existing, Some("PowerShell");
    "read" => crate::files::read, "Read numbered lines", Existing, None;
    "readlink" => crate::commands::pathname::run_readlink, "Display symbolic link targets", Ms, None;
    "realpath" => crate::commands::pathname::run_realpath, "Resolve canonical paths", Ms, None;
    "rg" => crate::search::run_rg, "Search files recursively", Existing, None;
    "rm" => crate::commands::fsops::rm::run, "Rm filesystem entries", Ms, None;
    "rmdir" => crate::commands::fsops::rmdir::run, "Rmdir filesystem entries", Ms, None;
    "search" => crate::search::run, "Search text in files", Existing, None;
    "seq" => crate::commands::seq::run, "Print numeric sequences", Ms, None;
    "sha1sum" => crate::commands::hashsum::run_sha1sum, "Compute sha1sum checksums", Ms, None;
    "sha224sum" => crate::commands::hashsum::run_sha224sum, "Compute sha224sum checksums", Ms, None;
    "sha256sum" => crate::commands::hashsum::run_sha256sum, "Compute sha256sum checksums", Ms, None;
    "sha384sum" => crate::commands::hashsum::run_sha384sum, "Compute sha384sum checksums", Ms, None;
    "sha512sum" => crate::commands::hashsum::run_sha512sum, "Compute sha512sum checksums", Ms, None;
    "sleep" => crate::commands::sleep::run, "Delay execution", Ms, Some("PowerShell");
    "sort" => crate::commands::sort::run, "Sort records", Ms, Some("PowerShell, Windows System32");
    "stat" => crate::commands::stat::run, "Report file status", Ms, None;
    "sum" => crate::commands::cksum::run_sum, "Compute sum checksums", Ms, None;
    "tac" => crate::commands::tac::run, "Concatenate files in reverse", Ms, None;
    "tail" => crate::text_commands::tail, "Print last lines", Existing, None;
    "tee" => crate::commands::tee::run, "Copy input to files and stdout", Ms, Some("PowerShell");
    "test" => crate::commands::test::run, "Evaluate an expression", Ms, None;
    "touch" => crate::commands::fsops::touch::run, "Touch filesystem entries", Ms, None;
    "tr" => crate::commands::tr::run, "Translate or delete bytes", Ms, None;
    "true" => crate::commands::truefalse::run_true, "Exit successfully", Ms, None;
    "truncate" => crate::commands::truncate::run, "Resize files", Ms, None;
    "unexpand" => crate::commands::expand::run_unexpand, "Convert spaces to expand", Ms, None;
    "uniq" => crate::commands::uniq::run, "Filter adjacent repeated records", Ms, None;
    "unlink" => crate::commands::fsops::unlink::run, "Unlink filesystem entries", Ms, None;
    "uptime" => crate::commands::sysinfo_cmds::run_uptime, "Report uptime system information", Ms, Some("PowerShell");
    "wc" => crate::commands::wc::run, "Count input lines, words and bytes", Ms, None;
    "xargs" => crate::commands::xargs::run, "Build and execute command lines", Ms, None;
    "yes" => crate::commands::yes::run, "Repeatedly display text", Ms, None;
}

pub fn find(name: &str) -> Option<&'static Utility> {
    UTILITIES
        .binary_search_by_key(&name, |utility| utility.name)
        .ok()
        .map(|index| &UTILITIES[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_matches_existing_commands() {
        let expected = [
            "[",
            "arch",
            "b2sum",
            "base32",
            "base64",
            "basename",
            "basenc",
            "cat",
            "cd",
            "cksum",
            "comm",
            "cp",
            "curl",
            "cut",
            "date",
            "df",
            "diff",
            "dirname",
            "du",
            "echo",
            "egrep",
            "env",
            "expand",
            "false",
            "fgrep",
            "find",
            "fold",
            "grep",
            "head",
            "hostname",
            "less",
            "link",
            "list",
            "ln",
            "ls",
            "md5sum",
            "mkdir",
            "mktemp",
            "mv",
            "nl",
            "nproc",
            "paste",
            "printenv",
            "printf",
            "ps",
            "pwd",
            "read",
            "readlink",
            "realpath",
            "rg",
            "rm",
            "rmdir",
            "search",
            "seq",
            "sha1sum",
            "sha224sum",
            "sha256sum",
            "sha384sum",
            "sha512sum",
            "sleep",
            "sort",
            "stat",
            "sum",
            "tac",
            "tail",
            "tee",
            "test",
            "touch",
            "tr",
            "true",
            "truncate",
            "unexpand",
            "uniq",
            "unlink",
            "uptime",
            "wc",
            "xargs",
            "yes",
        ];
        assert_eq!(
            UTILITIES.iter().map(|u| u.name).collect::<Vec<_>>(),
            expected
        );
        assert!(UTILITIES.windows(2).all(|w| w[0].name < w[1].name));
        assert!(expected.iter().all(|name| find(name).is_some()));
        assert!(find("missing").is_none());
    }
}
