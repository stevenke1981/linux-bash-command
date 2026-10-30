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

pub static UTILITIES: &[Utility] = &[
    Utility {
        name: "cat",
        run: crate::text_commands::cat,
        summary: "Concatenate files",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell"),
    },
    Utility {
        name: "cd",
        run: crate::cd_command::run,
        summary: "Resolve a directory",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell"),
    },
    Utility {
        name: "curl",
        run: crate::curl_command::run,
        summary: "Transfer data from URLs",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell 5.1"),
    },
    Utility {
        name: "df",
        run: crate::df_command::run,
        summary: "Report filesystem space",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "diff",
        run: crate::diff::run,
        summary: "Compare files",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell 5.1"),
    },
    Utility {
        name: "find",
        run: crate::find_command::find,
        summary: "Find files",
        origin: Origin::Existing,
        windows_conflict: Some("Windows"),
    },
    Utility {
        name: "grep",
        run: crate::grep_command::run,
        summary: "Search text",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "head",
        run: crate::text_commands::head,
        summary: "Print first lines",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "less",
        run: crate::less_command::run,
        summary: "Display files",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "list",
        run: crate::files::list,
        summary: "List files recursively",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "ls",
        run: crate::fs_commands::ls,
        summary: "List directory contents",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell"),
    },
    Utility {
        name: "ps",
        run: crate::ps_command::run,
        summary: "List processes",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell"),
    },
    Utility {
        name: "pwd",
        run: crate::fs_commands::pwd,
        summary: "Print working directory",
        origin: Origin::Existing,
        windows_conflict: Some("PowerShell"),
    },
    Utility {
        name: "read",
        run: crate::files::read,
        summary: "Read numbered lines",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "rg",
        run: crate::search::run_rg,
        summary: "Search files recursively",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "search",
        run: crate::search::run,
        summary: "Search text in files",
        origin: Origin::Existing,
        windows_conflict: None,
    },
    Utility {
        name: "tail",
        run: crate::text_commands::tail,
        summary: "Print last lines",
        origin: Origin::Existing,
        windows_conflict: None,
    },
];

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
            "cat", "cd", "curl", "df", "diff", "find", "grep", "head", "less", "list", "ls", "ps",
            "pwd", "read", "rg", "search", "tail",
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
