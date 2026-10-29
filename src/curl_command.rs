use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Method, Url};
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

const HELP: &str = "Usage: curl [OPTIONS] URL\n\nSend one HTTP(S) request and write its response body to standard output.\nThis is a small HTTP client, not a complete curl replacement.\n\nOptions:\n  -I, --head              Send HEAD and print response headers\n  -X, --request METHOD    Use this HTTP method\n  -H, --header HEADER     Add a request header (repeatable)\n  -d, --data DATA         Send UTF-8 request data; defaults to POST\n  -o, --output FILE       Write the response body to FILE ('-' means stdout)\n  -L, --location          Follow redirects\n  -f, --fail              Return 22 for HTTP status 400 or higher\n  -s, --silent            Suppress runtime error messages; no progress is shown\n  -m, --max-time SECONDS  Set a positive request timeout\n      --help              Show this help\n\nExit status: 0 on a completed request, 2 on usage/transport/I/O errors,\n22 when -f is used and the server returns status 400 or higher.\n";

#[derive(Default)]
struct Options {
    url: Option<String>,
    method: Option<String>,
    head: bool,
    headers: Vec<String>,
    data: Vec<String>,
    output: Option<PathBuf>,
    follow_redirects: bool,
    fail_http: bool,
    max_time: Option<Duration>,
    silent: bool,
}

fn parse(args: &[String]) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut positional_only = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if !positional_only && arg == "--help" {
            print!("{HELP}");
            return Ok(None);
        }
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && matches!(arg.as_str(), "-I" | "--head") {
            options.head = true;
        } else if !positional_only && matches!(arg.as_str(), "-L" | "--location") {
            options.follow_redirects = true;
        } else if !positional_only && matches!(arg.as_str(), "-f" | "--fail") {
            options.fail_http = true;
        } else if !positional_only && matches!(arg.as_str(), "-s" | "--silent") {
            options.silent = true;
        } else if !positional_only
            && matches!(
                arg.as_str(),
                "-X" | "--request"
                    | "-H"
                    | "--header"
                    | "-d"
                    | "--data"
                    | "-o"
                    | "--output"
                    | "-m"
                    | "--max-time"
            )
        {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(format!("{arg} requires a value"));
            };
            match arg.as_str() {
                "-X" | "--request" => options.method = Some(value.clone()),
                "-H" | "--header" => options.headers.push(value.clone()),
                "-d" | "--data" => options.data.push(value.clone()),
                "-o" | "--output" => options.output = Some(PathBuf::from(value)),
                "-m" | "--max-time" => {
                    let seconds = value.parse::<f64>().map_err(|_| {
                        "--max-time must be a positive number of seconds".to_string()
                    })?;
                    if !seconds.is_finite() || seconds <= 0.0 {
                        return Err("--max-time must be a positive number of seconds".to_string());
                    }
                    options.max_time =
                        Some(Duration::try_from_secs_f64(seconds).map_err(|_| {
                            "--max-time is outside the supported range".to_string()
                        })?);
                }
                _ => unreachable!(),
            }
        } else if !positional_only && arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else if options.url.replace(arg.clone()).is_some() {
            return Err("only one URL is supported".to_string());
        }
        index += 1;
    }
    if options.url.is_none() {
        return Err("missing URL".to_string());
    }
    Ok(Some(options))
}

fn validate_url(value: &str) -> Result<Url, String> {
    let url =
        Url::parse(value).map_err(|_| "expected a valid http:// or https:// URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("only http:// and https:// URLs are supported".to_string());
    }
    Ok(url)
}

fn validate_headers(headers: &[String]) -> Result<Vec<(HeaderName, HeaderValue)>, String> {
    headers
        .iter()
        .map(|header| {
            let (name, value) = header
                .split_once(':')
                .ok_or_else(|| "each -H header must have the form 'Name: value'".to_string())?;
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| "invalid request header name".to_string())?;
            let value = HeaderValue::from_str(value.trim())
                .map_err(|_| "invalid request header value".to_string())?;
            Ok((name, value))
        })
        .collect()
}

fn write_response(mut response: Response, options: &Options) -> Result<(), String> {
    if options.fail_http
        && (response.status().is_client_error() || response.status().is_server_error())
    {
        return Err("HTTP response status is 400 or higher".to_string());
    }
    let status = response.status();
    let headers = response.headers().clone();
    let is_head = options.head;
    let mut output: Box<dyn Write> = match options.output.as_deref() {
        Some(path) if path != std::path::Path::new("-") => {
            Box::new(File::create(path).map_err(|_| "cannot create output file".to_string())?)
        }
        _ => Box::new(io::stdout().lock()),
    };
    if is_head {
        let version = match response.version() {
            reqwest::Version::HTTP_09 => "HTTP/0.9",
            reqwest::Version::HTTP_10 => "HTTP/1.0",
            reqwest::Version::HTTP_11 => "HTTP/1.1",
            reqwest::Version::HTTP_2 => "HTTP/2",
            reqwest::Version::HTTP_3 => "HTTP/3",
            _ => "HTTP/?",
        };
        writeln!(
            output,
            "{} {} {}",
            version,
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .map_err(|_| "cannot write response headers".to_string())?;
        for (name, value) in &headers {
            writeln!(
                output,
                "{}: {}",
                name,
                String::from_utf8_lossy(value.as_bytes())
            )
            .map_err(|_| "cannot write response headers".to_string())?;
        }
        writeln!(output).map_err(|_| "cannot write response headers".to_string())?;
        return Ok(());
    }
    io::copy(&mut response, &mut output).map_err(|_| "cannot write response body".to_string())?;
    output
        .flush()
        .map_err(|_| "cannot flush response body".to_string())
}

pub fn run(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(Some(options)) => options,
        Ok(None) => return 0,
        Err(error) => {
            eprintln!("curl: {error}\nUse 'curl --help' for usage.");
            return 2;
        }
    };
    let url = match validate_url(options.url.as_deref().expect("validated URL")) {
        Ok(url) => url,
        Err(error) => {
            if !options.silent {
                eprintln!("curl: {error}");
            }
            return 2;
        }
    };
    let headers = match validate_headers(&options.headers) {
        Ok(headers) => headers,
        Err(error) => {
            if !options.silent {
                eprintln!("curl: {error}");
            }
            return 2;
        }
    };
    let method = match options.method.as_deref() {
        Some(method) => match Method::from_bytes(method.as_bytes()) {
            Ok(method) => method,
            Err(_) => {
                eprintln!("curl: invalid HTTP method");
                return 2;
            }
        },
        None if options.head => Method::HEAD,
        None if !options.data.is_empty() => Method::POST,
        None => Method::GET,
    };
    let client = match Client::builder()
        .redirect(if options.follow_redirects {
            reqwest::redirect::Policy::default()
        } else {
            reqwest::redirect::Policy::none()
        })
        .timeout(options.max_time)
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            if !options.silent {
                eprintln!("curl: cannot initialize HTTP client");
            }
            return 2;
        }
    };
    let mut request = client.request(method, url);
    let has_content_type = headers
        .iter()
        .any(|(name, _)| name == reqwest::header::CONTENT_TYPE);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if !options.data.is_empty() {
        if !has_content_type {
            request = request.header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            );
        }
        request = request.body(options.data.join("&"));
    }
    let response = match request.send() {
        Ok(response) => response,
        Err(_) => {
            if !options.silent {
                eprintln!("curl: request failed (connection, TLS, or timeout error)");
            }
            return 2;
        }
    };
    match write_response(response, &options) {
        Ok(()) => 0,
        Err(error) if options.fail_http && error == "HTTP response status is 400 or higher" => {
            if !options.silent {
                eprintln!("curl: server returned an error status (-f enabled)");
            }
            22
        }
        Err(error) => {
            if !options.silent {
                eprintln!("curl: {error}");
            }
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, validate_url};

    #[test]
    fn parses_get_and_post_options() {
        let get = parse(&["https://example.test".into()]).unwrap().unwrap();
        assert!(get.data.is_empty());
        let post = parse(&["-d".into(), "x=1".into(), "https://example.test".into()])
            .unwrap()
            .unwrap();
        assert_eq!(post.data, ["x=1"]);
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
    fn only_http_schemes_are_accepted() {
        assert!(validate_url("https://example.test").is_ok());
        assert!(validate_url("file:///secret").is_err());
        assert!(validate_url("not a url").is_err());
    }
}
