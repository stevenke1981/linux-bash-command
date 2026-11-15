use crate::common::args::{self, Item, OptSpec};
pub(super) const HELP:&str="Usage: tree [OPTION]... [DIRECTORY]...\nList directory contents in a tree. Symbolic links are displayed without traversal.\n  -a               include hidden files\n  -d               list directories only\n  -f               print full paths\n  -F               append file type indicators\n  -L LEVEL         descend at most LEVEL directories\n  -I PATTERN       exclude patterns separated by |\n  -P PATTERN       include patterns separated by |\n      --matchdirs  apply include patterns to directories\n      --ignore-case match patterns without ASCII case\n      --prune      omit empty directories\n      --filelimit=N refuse directories exceeding N entries\n      --noreport   omit the summary\n  -s               print sizes in bytes\n  -h               human-readable sizes\n      --si         use powers of 1000\n  -D               display modification times\n      --timefmt=FMT format modification time\n  -p               display permission bits\n      --dirsfirst  sort directories first\n      --filesfirst sort files first\n  -r               reverse sort order\n  -t               sort by modification time\n  -v               sort by version\n  -U               retain filesystem order\n      --sort=TYPE  name,version,size,mtime,ctime\n      --gitignore  honor .gitignore rules\n      --charset=TYPE ascii or utf-8\n  -n               do not color output\n  -J               output JSON\n  -i               omit indentation lines\n      --help       display help\n      --version    output version\nExit status: 0 success, 1 failure.\n";
pub(super) struct Options {
    pub all: bool,
    pub dirs: bool,
    pub full: bool,
    pub classify: bool,
    pub level: usize,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
    pub matchdirs: bool,
    pub ignore_case: bool,
    pub prune: bool,
    pub limit: usize,
    pub report: bool,
    pub size: u32,
    pub date: bool,
    pub timefmt: String,
    pub perms: bool,
    pub first: i8,
    pub reverse: bool,
    pub sort: String,
    pub gitignore: bool,
    pub ascii: bool,
    pub json: bool,
    pub flat: bool,
}
pub(super) fn parse(arguments: &[String]) -> Result<(Options, Vec<String>), String> {
    let specs = [
        OptSpec {
            id: "all",
            long: "",
            short: 'a',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "directories",
            long: "",
            short: 'd',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "full-path",
            long: "",
            short: 'f',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "classify",
            long: "",
            short: 'F',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "level",
            long: "",
            short: 'L',
            has_arg: args::HasArg::Required,
        },
        OptSpec {
            id: "exclude",
            long: "",
            short: 'I',
            has_arg: args::HasArg::Required,
        },
        OptSpec {
            id: "include",
            long: "",
            short: 'P',
            has_arg: args::HasArg::Required,
        },
        OptSpec::flag("matchdirs", '\0'),
        OptSpec::flag("ignore-case", '\0'),
        OptSpec::flag("prune", '\0'),
        OptSpec::value("filelimit", '\0'),
        OptSpec::flag("noreport", '\0'),
        OptSpec {
            id: "size",
            long: "",
            short: 's',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "human",
            long: "",
            short: 'h',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("si", '\0'),
        OptSpec {
            id: "date",
            long: "",
            short: 'D',
            has_arg: args::HasArg::No,
        },
        OptSpec::value("timefmt", '\0'),
        OptSpec {
            id: "permissions",
            long: "",
            short: 'p',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("dirsfirst", '\0'),
        OptSpec::flag("filesfirst", '\0'),
        OptSpec {
            id: "reverse",
            long: "",
            short: 'r',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "mtime",
            long: "",
            short: 't',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "version-sort",
            long: "",
            short: 'v',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "unsorted",
            long: "",
            short: 'U',
            has_arg: args::HasArg::No,
        },
        OptSpec::value("sort", '\0'),
        OptSpec::flag("gitignore", '\0'),
        OptSpec::value("charset", '\0'),
        OptSpec {
            id: "no-color",
            long: "",
            short: 'n',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "json",
            long: "",
            short: 'J',
            has_arg: args::HasArg::No,
        },
        OptSpec {
            id: "no-indent",
            long: "",
            short: 'i',
            has_arg: args::HasArg::No,
        },
        OptSpec::flag("help", '\0'),
        OptSpec::flag("version", '\0'),
    ];
    let mut o = Options {
        all: false,
        dirs: false,
        full: false,
        classify: false,
        level: usize::MAX,
        exclude: vec![],
        include: vec![],
        matchdirs: false,
        ignore_case: false,
        prune: false,
        limit: usize::MAX,
        report: true,
        size: 0,
        date: false,
        timefmt: "%b %e %H:%M".into(),
        perms: false,
        first: 0,
        reverse: false,
        sort: "name".into(),
        gitignore: false,
        ascii: false,
        json: false,
        flat: false,
    };
    let mut paths = vec![];
    for item in args::parse(arguments, &specs, true)? {
        match item {
            Item::Operand(s) => paths.push(s),
            Item::Opt(id, v) => match id {
                "all" => o.all = true,
                "directories" => o.dirs = true,
                "full-path" => o.full = true,
                "classify" => o.classify = true,
                "level" => o.level = positive(&v.unwrap())?,
                "filelimit" => o.limit = positive(&v.unwrap())?,
                "exclude" => o.exclude.extend(v.unwrap().split('|').map(str::to_owned)),
                "include" => o.include.extend(v.unwrap().split('|').map(str::to_owned)),
                "matchdirs" => o.matchdirs = true,
                "ignore-case" => o.ignore_case = true,
                "prune" => o.prune = true,
                "noreport" => o.report = false,
                "size" => {
                    if o.size == 0 {
                        o.size = 1;
                    }
                }
                "human" => o.size = 1024,
                "si" => o.size = 1000,
                "date" => o.date = true,
                "timefmt" => {
                    o.timefmt = v.unwrap();
                    o.date = true;
                }
                "permissions" => o.perms = true,
                "dirsfirst" => o.first = 1,
                "filesfirst" => o.first = -1,
                "reverse" => o.reverse = true,
                "mtime" => o.sort = "mtime".into(),
                "version-sort" => o.sort = "version".into(),
                "unsorted" => o.sort = "none".into(),
                "sort" => {
                    let s = v.unwrap();
                    if !["name", "version", "size", "mtime", "ctime"].contains(&s.as_str()) {
                        return Err(format!("invalid sort type: {s}"));
                    }
                    o.sort = s;
                }
                "gitignore" => o.gitignore = true,
                "charset" => {
                    o.ascii = match v.as_deref() {
                        Some("ascii") => true,
                        Some("utf-8") => false,
                        _ => return Err("invalid character set".into()),
                    };
                }
                "no-color" => {}
                "json" => o.json = true,
                "no-indent" => o.flat = true,
                "help" | "version" => return Ok((o, vec![format!("--{id}")])),
                _ => unreachable!(),
            },
        }
    }
    if paths.is_empty() {
        paths.push(".".into());
    }
    Ok((o, paths))
}
fn positive(s: &str) -> Result<usize, String> {
    s.parse()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid number: '{s}'"))
}
