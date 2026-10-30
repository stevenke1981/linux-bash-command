use crate::common::args::prepare_legacy;
use reqwest::blocking::{Client, ClientBuilder, Request, Response};
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::redirect::Policy;
use reqwest::{Method, NoProxy, Proxy, StatusCode, Url, Version};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const EXIT_USAGE: i32 = 2;
const EXIT_TRANSPORT: i32 = 2;
const EXIT_COULD_NOT_RESOLVE: i32 = 6;
const EXIT_CONNECT: i32 = 7;
const EXIT_HTTP_ERROR: i32 = 22;
const EXIT_WRITE: i32 = 23;
const EXIT_READ_LOCAL: i32 = 26;
const EXIT_TIMEOUT: i32 = 28;
const EXIT_TOO_MANY_REDIRECTS: i32 = 47;
const DEFAULT_MAX_REDIRECTS: usize = 50;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_USER_AGENT: &str = concat!("agentcmd-curl/", env!("CARGO_PKG_VERSION"));

const HELP: &str = r#"Usage: curl [OPTIONS] URL

Send one HTTP(S) request and write the response to standard output.
This is a subset of curl; unsupported options exit with status 2.

Output:
  -o, --output FILE        Write the body to FILE ('-' means stdout)
  -O, --remote-name        Write the body to the file named in the URL
      --create-dirs        Create missing parent directories for -o, -O and -D
  -D, --dump-header FILE   Write response headers to FILE ('-' means stdout)
  -i, --include            Print the status line and headers before the body
  -I, --head               Send HEAD and print only the response headers
  -w, --write-out FMT      Print FMT after the transfer; @FILE reads FMT.
                           Variables: http_code response_code url_effective
                           size_download time_total content_type redirect_url
                           num_redirects. Escapes: \n \r \t \\ and %%
  -v, --verbose            Print request and response headers to stderr
  -s, --silent             Hide errors and progress
  -S, --show-error         With -s, still show errors
      --no-progress-meter  Accepted; no progress meter is printed
  -#, --progress-bar       Accepted; no progress meter is printed

Request:
  -X, --request METHOD     Use this HTTP method
  -H, --header HEADER      Add a header; 'Name:' removes one, 'Name;' sends it empty
  -A, --user-agent VALUE   Set the User-Agent header
  -e, --referer URL        Set the Referer header
  -b, --cookie NAME=VALUE  Send a Cookie header (cookie files are not supported)
  -u, --user USER:PASS     HTTP basic authentication
  -k, --insecure           Accept invalid TLS certificates
      --compressed         Accepted; Accept-Encoding is not sent, so bodies are not decoded
      --http1.1            Use HTTP/1.1 only
      --http2              Accepted; HTTP/2 is used when the server negotiates it

Data (repeatable; joined with '&'):
  -d, --data DATA          Form data; @FILE and @- are read with CR/LF removed
      --data-raw DATA      Form data without @ processing
      --data-binary DATA   Form data; @FILE is sent byte for byte
      --data-urlencode S   content | =content | name=content | @file | name@file
      --json DATA          JSON body (@FILE allowed); sets Content-Type and Accept
  -G, --get                Append data to the URL query and use GET
  -F, --form               Not supported (multipart uploads); exits with 2

Network:
  -L, --location           Follow redirects
      --max-redirs N       Redirect limit with -L (default 50; -1 unlimited)
  -f, --fail               Exit 22 on HTTP status 400 or higher, without a body
      --fail-with-body     Like -f, but print the body before exiting 22
  -m, --max-time SECONDS   Total time limit
      --connect-timeout S  Connection time limit
      --retry N            Retry transport errors and 408, 429, 5xx responses
      --retry-delay S      Seconds between retries (default 1)
      --retry-max-time S   Stop retrying after S seconds in total
  -x, --proxy URL          HTTP proxy URL (without a scheme, http:// is assumed)
      --noproxy LIST       Hosts to bypass with -x; '*' disables proxies
      --url URL            Set the URL (same as the positional argument)
      --help               Show this help

Exit status: 0 on success; 2 for usage and transport errors; 6 when the host
cannot be resolved; 7 when the connection fails; 22 for -f or --fail-with-body
HTTP errors; 23 for local write errors; 26 for unreadable data files; 28 for
timeouts; 47 when --max-redirs is exceeded.
"#;

/// Option table: (short form, long name, takes a value). Long names are the canonical keys.
const OPTIONS: &[(Option<char>, &str, bool)] = &[
    (Some('I'), "head", false),
    (Some('i'), "include", false),
    (Some('X'), "request", true),
    (Some('H'), "header", true),
    (Some('A'), "user-agent", true),
    (Some('e'), "referer", true),
    (Some('b'), "cookie", true),
    (Some('u'), "user", true),
    (Some('k'), "insecure", false),
    (None, "compressed", false),
    (None, "http1.1", false),
    (None, "http2", false),
    (Some('d'), "data", true),
    (None, "data-raw", true),
    (None, "data-binary", true),
    (None, "data-urlencode", true),
    (None, "json", true),
    (Some('G'), "get", false),
    (Some('F'), "form", true),
    (None, "form-string", true),
    (Some('o'), "output", true),
    (Some('O'), "remote-name", false),
    (None, "create-dirs", false),
    (Some('D'), "dump-header", true),
    (Some('w'), "write-out", true),
    (Some('L'), "location", false),
    (None, "max-redirs", true),
    (Some('f'), "fail", false),
    (None, "fail-with-body", false),
    (Some('s'), "silent", false),
    (Some('S'), "show-error", false),
    (Some('v'), "verbose", false),
    (None, "no-progress-meter", false),
    (Some('#'), "progress-bar", false),
    (Some('x'), "proxy", true),
    (None, "noproxy", true),
    (Some('m'), "max-time", true),
    (None, "connect-timeout", true),
    (None, "retry", true),
    (None, "retry-delay", true),
    (None, "retry-max-time", true),
    (None, "url", true),
    (Some('h'), "help", false),
];

#[derive(Default)]
struct Options {
    url: Option<String>,
    method: Option<String>,
    head: bool,
    get: bool,
    headers: Vec<String>,
    data: Vec<DataPart>,
    json: bool,
    user_agent: Option<String>,
    referer: Option<String>,
    cookie: Option<String>,
    user: Option<(String, String)>,
    output: Option<String>,
    remote_name: bool,
    create_dirs: bool,
    dump_header: Option<String>,
    include: bool,
    write_out: Vec<Piece>,
    follow: bool,
    max_redirs: Option<usize>,
    fail: bool,
    fail_with_body: bool,
    silent: bool,
    show_error: bool,
    verbose: bool,
    insecure: bool,
    http1_only: bool,
    proxy: Option<String>,
    noproxy: Option<String>,
    max_time: Option<Duration>,
    connect_timeout: Option<Duration>,
    retry: u32,
    retry_delay: Option<Duration>,
    retry_max_time: Option<Duration>,
}

/// One `-d`-style piece. Pieces are joined with `&` into the request body.
struct DataPart {
    name: Option<String>,
    source: Source,
    encode: bool,
}

impl DataPart {
    fn plain(source: Source) -> Self {
        Self {
            name: None,
            source,
            encode: false,
        }
    }
}

enum Source {
    Text(String),
    File { path: String, strip_newlines: bool },
}

impl Source {
    fn read(&self) -> Result<Vec<u8>, Failure> {
        match self {
            Source::Text(text) => Ok(text.as_bytes().to_vec()),
            Source::File {
                path,
                strip_newlines,
            } => {
                let mut bytes = read_source(path).map_err(|_| {
                    fail(
                        EXIT_READ_LOCAL,
                        format!("failed to open or read local data from file: {path}"),
                    )
                })?;
                if *strip_newlines {
                    bytes.retain(|byte| !matches!(byte, b'\r' | b'\n'));
                }
                Ok(bytes)
            }
        }
    }
}

enum Piece {
    Text(String),
    Variable(Variable),
}

#[derive(Clone, Copy)]
enum Variable {
    HttpCode,
    UrlEffective,
    SizeDownload,
    TimeTotal,
    ContentType,
    RedirectUrl,
    NumRedirects,
}

/// A failure with the curl exit status it maps to.
struct Failure {
    code: i32,
    message: String,
}

fn fail(code: i32, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}

fn usage(message: impl Into<String>) -> Failure {
    fail(EXIT_USAGE, message)
}

/// Request headers from `-H`, kept in order. `removed` holds names suppressed by `Name:`.
#[derive(Default)]
struct CustomHeaders {
    set: Vec<(HeaderName, HeaderValue)>,
    removed: Vec<HeaderName>,
}

impl CustomHeaders {
    fn mentions(&self, name: &HeaderName) -> bool {
        self.removed.contains(name) || self.set.iter().any(|(set, _)| set == name)
    }
}

/// Everything needed to send the request, resolved before any network activity.
struct Plan {
    url: Url,
    method: Method,
    headers: HeaderMap,
    body: Option<Vec<u8>>,
    basic: Option<(String, String)>,
    output: Option<PathBuf>,
}

/// Redirect bookkeeping shared with the reqwest redirect policy.
struct RedirectState {
    limit: usize,
    followed: AtomicUsize,
    exceeded: AtomicBool,
}

impl RedirectState {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            followed: AtomicUsize::new(0),
            exceeded: AtomicBool::new(false),
        })
    }
}

/// Values available to `-w` after the transfer.
struct Metrics {
    status: u16,
    url: String,
    size_download: u64,
    content_type: String,
    redirect_url: String,
    redirects: usize,
}

pub fn run(args: &[String]) -> i32 {
    let start = Instant::now();
    let longs: Vec<&str> = OPTIONS.iter().map(|(_, long, _)| *long).collect();
    let value_longs: Vec<&str> = OPTIONS
        .iter()
        .filter(|(_, _, takes_value)| *takes_value)
        .map(|(_, long, _)| *long)
        .collect();
    let value_shorts: String = OPTIONS
        .iter()
        .filter(|(_, _, takes_value)| *takes_value)
        .filter_map(|(short, _, _)| *short)
        .collect();
    let expanded = match prepare_legacy(
        "curl",
        args,
        &longs,
        &value_longs,
        &value_shorts,
        EXIT_USAGE,
    ) {
        Ok(expanded) => expanded,
        Err(code) => return code,
    };
    let options = match parse(&expanded) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("curl: {error}\nTry 'curl --help' for more information.");
            return EXIT_USAGE;
        }
    };
    match execute(&options, start) {
        Ok(()) => 0,
        Err(failure) => {
            if !options.silent || options.show_error {
                eprintln!("curl: ({}) {}", failure.code, failure.message);
            }
            failure.code
        }
    }
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut positional_only = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        index += 1;
        if positional_only || arg == "-" || !arg.starts_with('-') {
            set_url(&mut options, arg.to_string())?;
        } else if arg == "--" {
            positional_only = true;
        } else if let Some(long) = arg.strip_prefix("--") {
            if !parse_long(&mut options, args, &mut index, arg, long)? {
                return Ok(None);
            }
        } else if !parse_short_cluster(&mut options, args, &mut index, arg)? {
            return Ok(None);
        }
    }
    if options.url.is_none() {
        return Err("missing URL".to_string());
    }
    Ok(Some(options))
}

/// Returns `Ok(false)` when help was printed.
fn parse_long(
    options: &mut Options,
    args: &[String],
    index: &mut usize,
    arg: &str,
    long: &str,
) -> Result<bool, String> {
    let (name, inline) = match long.split_once('=') {
        Some((name, value)) => (name, Some(value.to_string())),
        None => (long, None),
    };
    let (key, takes_value) = find_long(name).ok_or_else(|| format!("unknown option: {arg}"))?;
    let value = match (takes_value, inline) {
        (true, Some(value)) => Some(value),
        (true, None) => Some(next_value(args, index, arg)?),
        (false, None) => None,
        (false, Some(_)) => return Err(format!("option --{name} takes no value")),
    };
    apply(options, key, value)
}

/// Handles bundled short options such as `-sSL`, `-XPOST` and `-Lo FILE`.
/// Returns `Ok(false)` when help was printed.
fn parse_short_cluster(
    options: &mut Options,
    args: &[String],
    index: &mut usize,
    arg: &str,
) -> Result<bool, String> {
    let cluster = &arg[1..];
    for (offset, short) in cluster.char_indices() {
        let (key, takes_value) =
            find_short(short).ok_or_else(|| format!("unknown option: -{short}"))?;
        if !takes_value {
            if !apply(options, key, None)? {
                return Ok(false);
            }
            continue;
        }
        let attached = &cluster[offset + short.len_utf8()..];
        let value = if attached.is_empty() {
            next_value(args, index, arg)?
        } else {
            attached.to_string()
        };
        return apply(options, key, Some(value));
    }
    Ok(true)
}

fn find_short(short: char) -> Option<(&'static str, bool)> {
    OPTIONS
        .iter()
        .find(|(option, _, _)| *option == Some(short))
        .map(|(_, long, takes_value)| (*long, *takes_value))
}

fn find_long(name: &str) -> Option<(&'static str, bool)> {
    OPTIONS
        .iter()
        .find(|(_, long, _)| *long == name)
        .map(|(_, long, takes_value)| (*long, *takes_value))
}

fn next_value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    let value = args
        .get(*index)
        .ok_or_else(|| format!("option {option} requires a value"))?
        .clone();
    *index += 1;
    Ok(value)
}

fn set_url(options: &mut Options, value: String) -> Result<(), String> {
    if options.url.is_some() {
        return Err("only one URL is supported".to_string());
    }
    options.url = Some(value);
    Ok(())
}

/// Applies one option. Returns `Ok(false)` when help was printed.
fn apply(options: &mut Options, key: &str, value: Option<String>) -> Result<bool, String> {
    let value = value.unwrap_or_default();
    match key {
        "help" => {
            print!("{HELP}");
            return Ok(false);
        }
        "head" => options.head = true,
        "get" => options.get = true,
        "request" => options.method = Some(value),
        "header" => options.headers.push(value),
        "data" => options.data.push(DataPart::plain(data_source(value, true))),
        "data-raw" => options.data.push(DataPart::plain(Source::Text(value))),
        "data-binary" => options
            .data
            .push(DataPart::plain(data_source(value, false))),
        "data-urlencode" => options.data.push(url_encoded_part(&value)),
        "json" => {
            options.json = true;
            options
                .data
                .push(DataPart::plain(data_source(value, false)));
        }
        "form" | "form-string" => {
            return Err("multipart form uploads (-F/--form) are not supported".to_string())
        }
        "output" => {
            options.output = Some(value);
            options.remote_name = false;
        }
        "remote-name" => options.remote_name = true,
        "create-dirs" => options.create_dirs = true,
        "dump-header" => options.dump_header = Some(value),
        "include" => options.include = true,
        "write-out" => options.write_out = parse_write_out(&value)?,
        "location" => options.follow = true,
        "max-redirs" => options.max_redirs = Some(parse_redirects(&value)?),
        "fail" => options.fail = true,
        "fail-with-body" => {
            options.fail = true;
            options.fail_with_body = true;
        }
        "silent" => options.silent = true,
        "show-error" => options.show_error = true,
        "verbose" => options.verbose = true,
        "insecure" => options.insecure = true,
        "http1.1" => options.http1_only = true,
        "compressed" | "http2" | "progress-bar" | "no-progress-meter" => {}
        "user-agent" => options.user_agent = Some(value),
        "referer" => options.referer = Some(value),
        "cookie" => options.cookie = Some(parse_cookie(value)?),
        "user" => options.user = Some(parse_user(&value)?),
        "proxy" => options.proxy = Some(value),
        "noproxy" => options.noproxy = Some(value),
        "max-time" => options.max_time = nonzero(parse_seconds("--max-time", &value)?),
        "connect-timeout" => {
            options.connect_timeout = nonzero(parse_seconds("--connect-timeout", &value)?)
        }
        "retry" => options.retry = parse_retries(&value)?,
        "retry-delay" => options.retry_delay = Some(parse_seconds("--retry-delay", &value)?),
        "retry-max-time" => {
            options.retry_max_time = nonzero(parse_seconds("--retry-max-time", &value)?)
        }
        "url" => set_url(options, value)?,
        _ => return Err(format!("unsupported option: --{key}")),
    }
    Ok(true)
}

fn data_source(value: String, strip_newlines: bool) -> Source {
    if let Some(path) = value.strip_prefix('@') {
        return Source::File {
            path: path.to_string(),
            strip_newlines,
        };
    }
    Source::Text(value)
}

/// Parses `content`, `=content`, `name=content`, `@file` or `name@file`.
fn url_encoded_part(spec: &str) -> DataPart {
    let (name, rest, is_file) = match spec.find(['=', '@']) {
        Some(0) => (None, &spec[1..], spec.starts_with('@')),
        Some(position) => (
            Some(spec[..position].to_string()),
            &spec[position + 1..],
            spec.as_bytes()[position] == b'@',
        ),
        None => (None, spec, false),
    };
    let source = if is_file {
        Source::File {
            path: rest.to_string(),
            strip_newlines: false,
        }
    } else {
        Source::Text(rest.to_string())
    };
    DataPart {
        name,
        source,
        encode: true,
    }
}

fn parse_seconds(option: &str, value: &str) -> Result<Duration, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .ok_or_else(|| format!("{option} requires a non-negative number of seconds"))
}

/// Zero means "no limit" for timeouts, as in curl.
fn nonzero(duration: Duration) -> Option<Duration> {
    (!duration.is_zero()).then_some(duration)
}

fn parse_retries(value: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| "--retry requires a non-negative integer".to_string())
}

fn parse_redirects(value: &str) -> Result<usize, String> {
    match value.parse::<i64>() {
        Ok(-1) => Ok(usize::MAX),
        Ok(count) if count >= 0 => {
            usize::try_from(count).map_err(|_| "--max-redirs is too large".to_string())
        }
        _ => Err("--max-redirs requires an integer of -1 or more".to_string()),
    }
}

fn parse_cookie(value: String) -> Result<String, String> {
    if value.contains('=') {
        return Ok(value);
    }
    Err("-b/--cookie cookie files are not supported; use NAME=VALUE".to_string())
}

fn parse_user(value: &str) -> Result<(String, String), String> {
    value
        .split_once(':')
        .map(|(user, password)| (user.to_string(), password.to_string()))
        .ok_or_else(|| {
            "-u/--user requires USER:PASSWORD; interactive prompts are not supported".to_string()
        })
}

fn parse_write_out(value: &str) -> Result<Vec<Piece>, String> {
    let format = match value.strip_prefix('@') {
        Some(path) => {
            fs::read_to_string(path).map_err(|_| format!("cannot read write-out file: {path}"))?
        }
        None => value.to_string(),
    };
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('\\', Some(escaped @ ('n' | 'r' | 't' | '\\'))) => {
                chars.next();
                text.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => '\\',
                });
            }
            ('%', Some('%')) => {
                chars.next();
                text.push('%');
            }
            ('%', Some('{')) => {
                chars.next();
                let name = read_variable_name(&mut chars)?;
                if !text.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut text)));
                }
                pieces.push(Piece::Variable(variable_named(&name)?));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Ok(pieces)
}

fn read_variable_name(chars: &mut impl Iterator<Item = char>) -> Result<String, String> {
    let mut name = String::new();
    loop {
        match chars.next() {
            Some('}') => return Ok(name),
            Some(c) => name.push(c),
            None => return Err("unterminated %{...} in --write-out format".to_string()),
        }
    }
}

fn variable_named(name: &str) -> Result<Variable, String> {
    Ok(match name {
        "http_code" | "response_code" => Variable::HttpCode,
        "url_effective" => Variable::UrlEffective,
        "size_download" => Variable::SizeDownload,
        "time_total" => Variable::TimeTotal,
        "content_type" => Variable::ContentType,
        "redirect_url" => Variable::RedirectUrl,
        "num_redirects" => Variable::NumRedirects,
        other => return Err(format!("unsupported --write-out variable: %{{{other}}}")),
    })
}

fn validate_url(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "expected a valid http:// or https:// URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only http:// and https:// URLs are supported".to_string());
    }
    Ok(url)
}

fn execute(options: &Options, start: Instant) -> Result<(), Failure> {
    let plan = prepare(options)?;
    let redirects = RedirectState::new(options.max_redirs.unwrap_or(DEFAULT_MAX_REDIRECTS));
    let client = build_client(options, &redirects)?;
    let response = send_with_retry(&client, &plan, options, start, &redirects)?;
    let metrics = deliver(response, options, &plan, &redirects)?;
    write_out(&options.write_out, &metrics, start.elapsed())?;
    if options.fail && metrics.status >= 400 {
        return Err(fail(
            EXIT_HTTP_ERROR,
            format!("The requested URL returned error: {}", metrics.status),
        ));
    }
    Ok(())
}

fn prepare(options: &Options) -> Result<Plan, Failure> {
    let raw_url = options.url.as_deref().ok_or_else(|| usage("missing URL"))?;
    let mut url = validate_url(raw_url).map_err(usage)?;
    let body = match body_from_parts(&options.data)? {
        Some(bytes) if options.get => {
            append_query(&mut url, &bytes)?;
            None
        }
        body => body,
    };
    let custom = parse_custom_headers(&options.headers).map_err(usage)?;
    let headers = request_headers(options, &custom, body.is_some())?;
    let basic = options
        .user
        .clone()
        .filter(|_| !custom.mentions(&header::AUTHORIZATION));
    let method = choose_method(options, body.is_some())?;
    let output = output_path(options, &url)?;
    Ok(Plan {
        url,
        method,
        headers,
        body,
        basic,
        output,
    })
}

fn choose_method(options: &Options, has_body: bool) -> Result<Method, Failure> {
    let name = match options.method.as_deref() {
        Some(name) => name,
        None if options.head => "HEAD",
        None if has_body => "POST",
        None => "GET",
    };
    Method::from_bytes(name.as_bytes()).map_err(|_| usage("invalid HTTP method"))
}

fn body_from_parts(parts: &[DataPart]) -> Result<Option<Vec<u8>>, Failure> {
    if parts.is_empty() {
        return Ok(None);
    }
    let mut body = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            body.push(b'&');
        }
        if let Some(name) = &part.name {
            body.extend_from_slice(name.as_bytes());
            body.push(b'=');
        }
        let bytes = part.source.read()?;
        if part.encode {
            body.extend_from_slice(percent_encode(&bytes).as_bytes());
        } else {
            body.extend_from_slice(&bytes);
        }
    }
    Ok(Some(body))
}

/// Percent-encodes everything except RFC 3986 unreserved characters, as curl does.
fn percent_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn append_query(url: &mut Url, data: &[u8]) -> Result<(), Failure> {
    if data.is_empty() {
        return Ok(());
    }
    let data = std::str::from_utf8(data).map_err(|_| usage("-G data must be valid UTF-8"))?;
    let query = match url.query() {
        Some(existing) if !existing.is_empty() => format!("{existing}&{data}"),
        _ => data.to_string(),
    };
    url.set_query(Some(&query));
    Ok(())
}

fn output_path(options: &Options, url: &Url) -> Result<Option<PathBuf>, Failure> {
    if options.remote_name {
        return remote_name(url).map(Some);
    }
    Ok(match options.output.as_deref() {
        None | Some("-") => None,
        Some(path) => Some(PathBuf::from(path)),
    })
}

/// Uses the last URL path segment, refusing names that are empty or unsafe on Windows.
fn remote_name(url: &Url) -> Result<PathBuf, Failure> {
    let name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    let unusable = matches!(name, "" | "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c));
    if unusable {
        return Err(fail(
            EXIT_WRITE,
            "cannot derive a file name from the URL; use -o FILE",
        ));
    }
    Ok(PathBuf::from(name))
}

/// Splits one `-H` value. `Some(value)` sets a header; `None` removes it.
fn split_header(line: &str) -> Result<(&str, Option<&str>), String> {
    if let Some((name, value)) = line.split_once(':') {
        let value = value.trim();
        return Ok((name, (!value.is_empty()).then_some(value)));
    }
    line.strip_suffix(';')
        .map(|name| (name, Some("")))
        .ok_or_else(|| {
            format!("each -H header must have the form 'Name: value', 'Name:' or 'Name;': {line}")
        })
}

fn parse_custom_headers(lines: &[String]) -> Result<CustomHeaders, String> {
    let mut custom = CustomHeaders::default();
    for line in lines {
        let (name, value) = split_header(line)?;
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| format!("invalid request header name in -H '{line}'"))?;
        match value {
            Some(value) => {
                let value = HeaderValue::from_str(value)
                    .map_err(|_| format!("invalid request header value in -H '{line}'"))?;
                custom.set.push((name, value));
            }
            None => {
                custom.set.retain(|(set, _)| *set != name);
                custom.removed.push(name);
            }
        }
    }
    Ok(custom)
}

/// Builds the final header map: `-H` entries first, then curl-style defaults the user did not override.
fn request_headers(
    options: &Options,
    custom: &CustomHeaders,
    has_body: bool,
) -> Result<HeaderMap, Failure> {
    let mut map = HeaderMap::new();
    for (name, value) in &custom.set {
        map.append(name.clone(), value.clone());
    }
    let accept = if options.json {
        "application/json"
    } else {
        "*/*"
    };
    let content_type = if options.json {
        "application/json"
    } else {
        "application/x-www-form-urlencoded"
    };
    let defaults = [
        (
            header::USER_AGENT,
            Some(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT)),
        ),
        (header::ACCEPT, Some(accept)),
        (header::REFERER, options.referer.as_deref()),
        (header::COOKIE, options.cookie.as_deref()),
        (header::CONTENT_TYPE, has_body.then_some(content_type)),
    ];
    for (name, value) in defaults {
        let Some(value) = value.filter(|_| !custom.mentions(&name)) else {
            continue;
        };
        let value = HeaderValue::from_str(value)
            .map_err(|_| usage(format!("invalid value for header {name}")))?;
        map.append(name, value);
    }
    // reqwest always adds `Accept: */*` as a client default and offers no way to drop it,
    // so `-H 'Accept:'` is sent as an empty Accept header (curl omits the header entirely).
    let accept_set = custom.set.iter().any(|(name, _)| *name == header::ACCEPT);
    if custom.removed.contains(&header::ACCEPT) && !accept_set {
        map.append(header::ACCEPT, HeaderValue::from_static(""));
    }
    Ok(map)
}

fn build_client(options: &Options, redirects: &Arc<RedirectState>) -> Result<Client, Failure> {
    let builder = Client::builder()
        .redirect(redirect_policy(options, redirects))
        .timeout(options.max_time)
        .connect_timeout(options.connect_timeout)
        .danger_accept_invalid_certs(options.insecure);
    let builder = if options.http1_only {
        builder.http1_only()
    } else {
        builder
    };
    let builder = proxy_settings(builder, options)?;
    builder
        .build()
        .map_err(|_| fail(EXIT_USAGE, "cannot initialize HTTP client"))
}

fn redirect_policy(options: &Options, redirects: &Arc<RedirectState>) -> Policy {
    if !options.follow {
        return Policy::none();
    }
    let state = Arc::clone(redirects);
    Policy::custom(move |attempt| {
        if state.followed.load(Ordering::Relaxed) >= state.limit {
            state.exceeded.store(true, Ordering::Relaxed);
            return attempt.error("too many redirects");
        }
        state.followed.fetch_add(1, Ordering::Relaxed);
        attempt.follow()
    })
}

fn proxy_settings(builder: ClientBuilder, options: &Options) -> Result<ClientBuilder, Failure> {
    if options.noproxy.as_deref() == Some("*") {
        return Ok(builder.no_proxy());
    }
    let Some(proxy) = options.proxy.as_deref() else {
        if options.noproxy.is_some() {
            return Err(usage(
                "--noproxy requires -x/--proxy, or '*' to disable proxies",
            ));
        }
        return Ok(builder);
    };
    let proxy_url = if proxy.contains("://") {
        proxy.to_string()
    } else {
        format!("http://{proxy}")
    };
    let no_proxy = options.noproxy.as_deref().and_then(NoProxy::from_string);
    let proxy = Proxy::all(proxy_url)
        .map_err(|_| usage("invalid proxy URL"))?
        .no_proxy(no_proxy);
    Ok(builder.proxy(proxy))
}

fn send_with_retry(
    client: &Client,
    plan: &Plan,
    options: &Options,
    start: Instant,
    redirects: &RedirectState,
) -> Result<Response, Failure> {
    let mut retries_left = options.retry;
    loop {
        let result = send_once(client, plan, options);
        let retryable = match &result {
            Ok(response) => is_transient(response.status()),
            Err(_) => !redirects.exceeded.load(Ordering::Relaxed),
        };
        let budget_left = match options.retry_max_time {
            Some(max) => start.elapsed() < max,
            None => true,
        };
        if !retryable || retries_left == 0 || !budget_left {
            return result.map_err(|error| transport_failure(&error, options, redirects));
        }
        retries_left -= 1;
        thread::sleep(options.retry_delay.unwrap_or(DEFAULT_RETRY_DELAY));
    }
}

fn is_transient(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429) || status.is_server_error()
}

fn transport_failure(
    error: &reqwest::Error,
    options: &Options,
    redirects: &RedirectState,
) -> Failure {
    if redirects.exceeded.load(Ordering::Relaxed) {
        let limit = options.max_redirs.unwrap_or(DEFAULT_MAX_REDIRECTS);
        return fail(
            EXIT_TOO_MANY_REDIRECTS,
            format!("maximum ({limit}) redirects followed"),
        );
    }
    if error.is_timeout() {
        return fail(EXIT_TIMEOUT, "operation timed out");
    }
    if error.is_connect() {
        let host = error
            .url()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default();
        if is_dns_failure(error) {
            return fail(
                EXIT_COULD_NOT_RESOLVE,
                format!("Could not resolve host: {host}"),
            );
        }
        return fail(EXIT_CONNECT, format!("Failed to connect to {host}"));
    }
    fail(
        EXIT_TRANSPORT,
        "request failed (connection, TLS, or protocol error)",
    )
}

/// True when a connection failure came from name resolution rather than from connecting.
/// The resolver's wording is only available in the error chain.
fn is_dns_failure(error: &reqwest::Error) -> bool {
    const DNS_MARKERS: [&str; 5] = [
        "dns error",
        "failed to lookup address",
        "name or service not known",
        "no such host",
        "nodename nor servname",
    ];
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        let text = current.to_string().to_ascii_lowercase();
        if DNS_MARKERS.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        source = current.source();
    }
    false
}

fn send_once(client: &Client, plan: &Plan, options: &Options) -> Result<Response, reqwest::Error> {
    let mut builder = client
        .request(plan.method.clone(), plan.url.clone())
        .headers(plan.headers.clone());
    if let Some(body) = &plan.body {
        builder = builder.body(body.clone());
    }
    if let Some((user, password)) = &plan.basic {
        builder = builder.basic_auth(user, Some(password));
    }
    let request = builder.build()?;
    if options.verbose {
        print_request(&request, plan.body.as_ref().map(Vec::len));
    }
    let response = client.execute(request)?;
    if options.verbose {
        print_response(&response);
    }
    Ok(response)
}

fn deliver(
    mut response: Response,
    options: &Options,
    plan: &Plan,
    redirects: &RedirectState,
) -> Result<Metrics, Failure> {
    let status = response.status();
    let mut metrics = Metrics {
        status: status.as_u16(),
        url: response.url().to_string(),
        size_download: 0,
        content_type: header_text(response.headers(), &header::CONTENT_TYPE),
        redirect_url: redirect_target(&response, options.follow),
        redirects: redirects.followed.load(Ordering::Relaxed),
    };
    if let Some(target) = options.dump_header.as_deref() {
        dump_headers(target, options.create_dirs, &response)?;
    }
    let print_head = options.include || options.head;
    let http_error = status.is_client_error() || status.is_server_error();
    let body_wanted = !(options.fail && http_error) || options.fail_with_body;
    if !print_head && !body_wanted {
        return Ok(metrics);
    }
    let mut sink = open_sink(plan.output.as_deref(), options.create_dirs)?;
    if print_head {
        write_head(sink.as_mut(), &response)
            .map_err(|_| fail(EXIT_WRITE, "cannot write response headers"))?;
    }
    if body_wanted {
        metrics.size_download = io::copy(&mut response, &mut sink).map_err(copy_failure)?;
    }
    sink.flush()
        .map_err(|_| fail(EXIT_WRITE, "cannot write response body"))?;
    Ok(metrics)
}

fn dump_headers(target: &str, create_dirs: bool, response: &Response) -> Result<(), Failure> {
    let path = (target != "-").then(|| PathBuf::from(target));
    let mut sink = open_sink(path.as_deref(), create_dirs)?;
    write_head(sink.as_mut(), response)
        .map_err(|_| fail(EXIT_WRITE, "cannot write response headers"))?;
    sink.flush()
        .map_err(|_| fail(EXIT_WRITE, "cannot write response headers"))
}

fn copy_failure(error: io::Error) -> Failure {
    if error.kind() == io::ErrorKind::TimedOut {
        return fail(EXIT_TIMEOUT, "operation timed out");
    }
    fail(EXIT_WRITE, "failed writing response body")
}

fn open_sink(path: Option<&Path>, create_dirs: bool) -> Result<Box<dyn Write>, Failure> {
    let Some(path) = path else {
        return Ok(Box::new(io::stdout().lock()));
    };
    if create_dirs {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|_| fail(EXIT_WRITE, "cannot create output directory"))?;
        }
    }
    let file = File::create(path).map_err(|_| {
        fail(
            EXIT_WRITE,
            format!("cannot create output file: {}", path.display()),
        )
    })?;
    Ok(Box::new(file))
}

fn redirect_target(response: &Response, follow: bool) -> String {
    if follow || !response.status().is_redirection() {
        return String::new();
    }
    response
        .headers()
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|location| response.url().join(location).ok())
        .map(|url| url.to_string())
        .unwrap_or_default()
}

fn write_out(pieces: &[Piece], metrics: &Metrics, elapsed: Duration) -> Result<(), Failure> {
    if pieces.is_empty() {
        return Ok(());
    }
    let text = render(pieces, metrics, elapsed);
    let mut stdout = io::stdout();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|_| fail(EXIT_WRITE, "cannot write --write-out output"))
}

fn render(pieces: &[Piece], metrics: &Metrics, elapsed: Duration) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => text.clone(),
            Piece::Variable(variable) => variable_value(*variable, metrics, elapsed),
        })
        .collect()
}

fn variable_value(variable: Variable, metrics: &Metrics, elapsed: Duration) -> String {
    match variable {
        Variable::HttpCode => metrics.status.to_string(),
        Variable::UrlEffective => metrics.url.clone(),
        Variable::SizeDownload => metrics.size_download.to_string(),
        Variable::TimeTotal => format!("{:.6}", elapsed.as_secs_f64()),
        Variable::ContentType => metrics.content_type.clone(),
        Variable::RedirectUrl => metrics.redirect_url.clone(),
        Variable::NumRedirects => metrics.redirects.to_string(),
    }
}

fn write_head(out: &mut dyn Write, response: &Response) -> io::Result<()> {
    writeln!(
        out,
        "{}",
        status_line(response.version(), response.status())
    )?;
    for (name, value) in response.headers() {
        writeln!(out, "{name}: {}", header_value_text(value))?;
    }
    writeln!(out)
}

fn status_line(version: Version, status: StatusCode) -> String {
    let version = match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    };
    match status.canonical_reason() {
        Some(reason) => format!("{version} {} {reason}", status.as_u16()),
        None => format!("{version} {}", status.as_u16()),
    }
}

fn print_request(request: &Request, body_len: Option<usize>) {
    let url = request.url();
    let mut target = url.path().to_string();
    if let Some(query) = url.query() {
        target = format!("{target}?{query}");
    }
    eprintln!("> {} {target} HTTP/1.1", request.method());
    let headers = request.headers();
    if !headers.contains_key(header::HOST) {
        eprintln!("> Host: {}", host_header(url));
    }
    for (name, value) in headers {
        eprintln!("> {name}: {}", header_value_text(value));
    }
    if let Some(len) = body_len.filter(|_| !headers.contains_key(header::CONTENT_LENGTH)) {
        eprintln!("> Content-Length: {len}");
    }
    eprintln!(">");
}

fn print_response(response: &Response) {
    eprintln!("< {}", status_line(response.version(), response.status()));
    for (name, value) in response.headers() {
        eprintln!("< {name}: {}", header_value_text(value));
    }
    eprintln!("<");
}

fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

fn header_text(headers: &HeaderMap, name: &HeaderName) -> String {
    headers.get(name).map(header_value_text).unwrap_or_default()
}

fn header_value_text(value: &HeaderValue) -> String {
    String::from_utf8_lossy(value.as_bytes()).into_owned()
}

fn read_source(path: &str) -> io::Result<Vec<u8>> {
    if path != "-" {
        return fs::read(path);
    }
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{parse, percent_encode, split_header, validate_url};

    #[test]
    fn parses_get_and_post_options() {
        let get = parse(&["https://example.test".into()]).unwrap().unwrap();
        assert!(get.data.is_empty());
        let post = parse(&["-d".into(), "x=1".into(), "https://example.test".into()])
            .unwrap()
            .unwrap();
        assert_eq!(post.data.len(), 1);
        assert!(parse(&[]).is_err());
        assert!(parse(&["-m".into(), "1e100".into(), "https://example.test".into()]).is_err());
        assert!(parse(&["-m".into(), "NaN".into(), "https://example.test".into()]).is_err());
        assert!(
            parse(&["-s".into(), "https://example.test".into()])
                .unwrap()
                .unwrap()
                .silent
        );
    }

    #[test]
    fn parses_bundled_and_attached_short_options() {
        let bundled = parse(&["-sSLXPOST".into(), "https://example.test".into()])
            .unwrap()
            .unwrap();
        assert!(bundled.silent && bundled.show_error && bundled.follow);
        assert_eq!(bundled.method.as_deref(), Some("POST"));
        let output = parse(&[
            "-Lo".into(),
            "out.txt".into(),
            "https://example.test".into(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(output.output.as_deref(), Some("out.txt"));
        assert!(parse(&["-Z".into(), "https://example.test".into()]).is_err());
    }

    #[test]
    fn only_http_schemes_are_accepted() {
        assert!(validate_url("https://example.test").is_ok());
        assert!(validate_url("file:///secret").is_err());
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn header_removal_and_empty_forms() {
        assert_eq!(split_header("Accept:").unwrap(), ("Accept", None));
        assert_eq!(split_header("X-E;").unwrap(), ("X-E", Some("")));
        assert_eq!(split_header("X-A: b").unwrap(), ("X-A", Some("b")));
        assert!(split_header("broken").is_err());
    }

    #[test]
    fn percent_encodes_like_curl() {
        assert_eq!(percent_encode(b"a b&c~_.-"), "a%20b%26c~_.-");
    }
}
