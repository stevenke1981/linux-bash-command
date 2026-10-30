//! `factor`: print the prime factors of each number.
//!
//! Numbers are `u128`. Small factors are removed by trial division; the rest is split with
//! Pollard's rho, and primality is decided by Miller-Rabin with the first 25 primes as bases.
//! Those bases make the test deterministic below 3.3 * 10^24; above that it is a strong
//! probable-prime test with negligible error. Values that do not fit `u128` are rejected.

use std::io::{self, BufRead, Write};

use crate::common::args::{report_usage, Item, OptSpec, Parser};
use crate::common::error::{io_error_text, print_version, EXIT_FAIL, EXIT_OK};
use crate::common::io::{finish_output, is_broken_pipe, stdout_writer};

const CMD: &str = "factor";

const SPECS: &[OptSpec] = &[
    OptSpec::flag("exponents", Some('h'), Some("exponents")),
    OptSpec::flag("help", None, Some("help")),
    OptSpec::flag("version", None, Some("version")),
];

const HELP: &str = "Usage: factor [NUMBER]...\n  or:  factor OPTION\n\
Print the prime factors of each specified integer NUMBER on standard output.\n\
With no NUMBER, read standard input.\n\n\
  -h, --exponents   print repeated factors in form p^e\n\
      --help        display this help and exit\n\
      --version     output version information and exit\n\n\
Exit status: 0 if all numbers were factored, 1 otherwise.\n";

/// Primes used for trial division and as Miller-Rabin bases (the first 25 primes).
const PRIMES: [u128; 25] = [
    2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
];

pub fn run(args: &[String]) -> i32 {
    let items = match Parser::new(CMD, SPECS).parse(args) {
        Ok(items) => items,
        Err(error) => return report_usage(CMD, &error, EXIT_FAIL),
    };
    let mut exponents = false;
    let mut operands = Vec::new();
    for item in &items {
        match item {
            Item::Opt { id: "help", .. } => {
                print!("{HELP}");
                return EXIT_OK;
            }
            Item::Opt { id: "version", .. } => return print_version(CMD),
            Item::Opt {
                id: "exponents", ..
            } => exponents = true,
            Item::Opt { .. } => {}
            Item::Operand(text) => operands.push(text.clone()),
        }
    }
    let mut out = stdout_writer();
    let mut status = EXIT_OK;
    let written = if operands.is_empty() {
        factor_stream(&mut out, exponents, &mut status)
    } else {
        operands
            .iter()
            .try_for_each(|text| factor_token(&mut out, text.as_bytes(), exponents, &mut status))
    };
    if let Err(error) = written {
        if is_broken_pipe(&error) {
            return EXIT_OK;
        }
        eprintln!("{CMD}: write error: {}", io_error_text(&error));
        return EXIT_FAIL;
    }
    let flushed = finish_output(CMD, &mut out);
    if flushed != EXIT_OK {
        return flushed;
    }
    status
}

/// Factors every whitespace-separated token of standard input.
fn factor_stream(out: &mut impl Write, exponents: bool, status: &mut i32) -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        for token in line.split(|byte| byte.is_ascii_whitespace()) {
            if !token.is_empty() {
                factor_token(out, token, exponents, status)?;
            }
        }
    }
}

/// Factors one token, reporting a bad token on stderr and setting `status`.
fn factor_token(
    out: &mut impl Write,
    token: &[u8],
    exponents: bool,
    status: &mut i32,
) -> io::Result<()> {
    let text = String::from_utf8_lossy(token).into_owned();
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        eprintln!("{CMD}: '{text}' is not a valid positive integer");
        *status = EXIT_FAIL;
        return Ok(());
    }
    let Ok(number) = text.parse::<u128>() else {
        eprintln!("{CMD}: '{text}' is too large");
        *status = EXIT_FAIL;
        return Ok(());
    };
    let factors = prime_factors(number);
    write!(out, "{number}:")?;
    for (prime, count) in group(&factors) {
        if exponents && count > 1 {
            write!(out, " {prime}^{count}")?;
        } else {
            for _ in 0..(if exponents { 1 } else { count }) {
                write!(out, " {prime}")?;
            }
        }
    }
    out.write_all(b"\n")
}

/// Collapses a sorted factor list into `(prime, multiplicity)` pairs.
fn group(sorted: &[u128]) -> Vec<(u128, usize)> {
    let mut groups: Vec<(u128, usize)> = Vec::new();
    for &prime in sorted {
        match groups.last_mut() {
            Some((last, count)) if *last == prime => *count += 1,
            _ => groups.push((prime, 1)),
        }
    }
    groups
}

/// All prime factors of `n` in ascending order, with repetition. `0` and `1` have none.
fn prime_factors(n: u128) -> Vec<u128> {
    let mut factors = Vec::new();
    if n < 2 {
        return factors;
    }
    let mut rest = n;
    for &prime in &PRIMES {
        while rest.is_multiple_of(prime) {
            factors.push(prime);
            rest /= prime;
        }
    }
    split(rest, &mut factors);
    factors.sort_unstable();
    factors
}

/// Appends the prime factors of `n`, which has no factor below 100.
fn split(n: u128, factors: &mut Vec<u128>) {
    if n == 1 {
        return;
    }
    if is_prime(n) {
        factors.push(n);
        return;
    }
    let divisor = pollard_rho(n);
    split(divisor, factors);
    split(n / divisor, factors);
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// `(a + b) mod n` for `a, b < n`, without overflowing `u128`.
fn add_mod(a: u128, b: u128, n: u128) -> u128 {
    if a >= n - b {
        a - (n - b)
    } else {
        a + b
    }
}

/// `(a * b) mod n` for `a, b < n`. Uses a double-width product when `n` fits in 64 bits, and
/// double-and-add otherwise.
fn mul_mod(a: u128, b: u128, n: u128) -> u128 {
    if n <= u128::from(u64::MAX) {
        return (a * b) % n;
    }
    let (mut result, mut base, mut power) = (0, a % n, b);
    while power > 0 {
        if power & 1 == 1 {
            result = add_mod(result, base, n);
        }
        base = add_mod(base, base, n);
        power >>= 1;
    }
    result
}

fn pow_mod(mut base: u128, mut exponent: u128, n: u128) -> u128 {
    let mut result = 1 % n;
    base %= n;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = mul_mod(result, base, n);
        }
        base = mul_mod(base, base, n);
        exponent >>= 1;
    }
    result
}

/// Miller-Rabin over the first 25 primes. `n` must be odd and greater than 97.
fn is_prime(n: u128) -> bool {
    if n < 2 {
        return false;
    }
    for &prime in &PRIMES {
        if n.is_multiple_of(prime) {
            return n == prime;
        }
    }
    let mut odd = n - 1;
    let mut twos = 0;
    while odd.is_multiple_of(2) {
        odd /= 2;
        twos += 1;
    }
    PRIMES.iter().all(|&base| {
        let mut x = pow_mod(base, odd, n);
        if x == 1 || x == n - 1 {
            return true;
        }
        for _ in 1..twos {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                return true;
            }
        }
        false
    })
}

/// A non-trivial divisor of the composite `n`, found by Pollard's rho with a varying constant.
fn pollard_rho(n: u128) -> u128 {
    if n.is_multiple_of(2) {
        return 2;
    }
    for constant in 1u128.. {
        let step = |x: u128| add_mod(mul_mod(x, x, n), constant % n, n);
        let (mut slow, mut fast, mut divisor) = (2u128, 2u128, 1u128);
        while divisor == 1 {
            slow = step(slow);
            fast = step(step(fast));
            divisor = gcd(slow.abs_diff(fast), n);
        }
        if divisor != n {
            return divisor;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factors_small_numbers_with_repetition() {
        assert_eq!(prime_factors(360), vec![2, 2, 2, 3, 3, 5]);
        assert_eq!(prime_factors(97), vec![97]);
        assert!(prime_factors(1).is_empty());
        assert!(prime_factors(0).is_empty());
    }

    #[test]
    fn splits_products_of_large_primes() {
        // 2^61 - 1 and 2^31 - 1 are both prime.
        let n = ((1u128 << 61) - 1) * ((1u128 << 31) - 1);
        assert_eq!(prime_factors(n), vec![(1u128 << 31) - 1, (1u128 << 61) - 1]);
    }

    #[test]
    fn handles_values_beyond_64_bits() {
        // 2^127 - 1 is a Mersenne prime.
        let mersenne = (1u128 << 127) - 1;
        assert_eq!(prime_factors(mersenne), vec![mersenne]);
        assert_eq!(prime_factors(1u128 << 100), vec![2; 100]);
    }

    #[test]
    fn miller_rabin_detects_strong_pseudoprimes() {
        assert!(is_prime(1_000_000_007));
        assert!(!is_prime(3_215_031_751)); // strong pseudoprime to bases 2, 3, 5 and 7
        assert!(!is_prime(3_825_123_056_546_413_051)); // strong pseudoprime to the first 9 primes
    }

    #[test]
    fn exponent_form_groups_repeats() {
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        factor_token(&mut out, b"12", true, &mut status).unwrap();
        factor_token(&mut out, b"12", false, &mut status).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "12: 2^2 3\n12: 2 2 3\n");
        assert_eq!(status, EXIT_OK);
    }

    #[test]
    fn invalid_tokens_fail_but_do_not_stop() {
        let mut out = Vec::new();
        let mut status = EXIT_OK;
        factor_token(&mut out, b"-5", false, &mut status).unwrap();
        factor_token(&mut out, b"7", false, &mut status).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "7: 7\n");
        assert_eq!(status, EXIT_FAIL);
    }
}
