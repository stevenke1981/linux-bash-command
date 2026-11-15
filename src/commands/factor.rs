use crate::common::{
    args::{self, Item, OptSpec},
    io,
};
use std::collections::HashSet;

const HELP: &str = "Usage: factor [OPTION]... [NUMBER]...\nPrint the prime factors of each NUMBER.\n  -h, --exponents  print repeated factors in exponent form\n      --help       display this help\n      --version    output version information\n";

fn add_mod(a: u128, b: u128, n: u128) -> u128 {
    if a >= n - b {
        a - (n - b)
    } else {
        a + b
    }
}

fn mul_mod(mut a: u128, mut b: u128, n: u128) -> u128 {
    if let Some(product) = a.checked_mul(b) {
        return product % n;
    }
    let mut result = 0;
    a %= n;
    while b != 0 {
        if b & 1 != 0 {
            result = add_mod(result, a, n);
        }
        b >>= 1;
        if b != 0 {
            a = add_mod(a, a, n);
        }
    }
    result
}

fn pow_mod(mut a: u128, mut b: u128, n: u128) -> u128 {
    let mut result = 1;
    a %= n;
    while b != 0 {
        if b & 1 != 0 {
            result = mul_mod(result, a, n);
        }
        b >>= 1;
        if b != 0 {
            a = mul_mod(a, a, n);
        }
    }
    result
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

// This is a deterministic test for u64, and a reproducible screening test for
// larger values. Large candidates must also receive a Pocklington certificate.
fn probable_prime(n: u128) -> bool {
    const SMALL: [u128; 25] = [
        2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89,
        97,
    ];
    const U64_BASES: [u128; 7] = [2, 325, 9375, 28178, 450775, 9780504, 1795265022];
    if n < 2 {
        return false;
    }
    for p in SMALL {
        if n.is_multiple_of(p) {
            return n == p;
        }
    }
    let s = (n - 1).trailing_zeros();
    let d = (n - 1) >> s;
    let witnesses: &[u128] = if n <= u64::MAX as u128 {
        &U64_BASES
    } else {
        &SMALL
    };
    'witness: for &a in witnesses {
        let a = a % n;
        if a == 0 {
            continue;
        }
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 1..s {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

fn rho(n: u128) -> u128 {
    if n.is_multiple_of(2) {
        return 2;
    }
    let mut c = 1;
    loop {
        let mut x = 2;
        let mut y = 2;
        let mut divisor = 1;
        while divisor == 1 {
            x = add_mod(mul_mod(x, x, n), c, n);
            y = add_mod(mul_mod(y, y, n), c, n);
            y = add_mod(mul_mod(y, y, n), c, n);
            divisor = gcd(x.abs_diff(y), n);
        }
        if divisor != n {
            return divisor;
        }
        c = if c + 1 == n { 1 } else { c + 1 };
    }
}

// With a complete, recursively certified factorization of n-1, finding one
// witness satisfying Pocklington's conditions proves n prime. A nontrivial gcd
// or failed Fermat congruence proves compositeness; no candidate is accepted
// merely because the witness search has run for a fixed number of iterations.
fn certified_prime(n: u128, certified: &mut HashSet<u128>) -> bool {
    if certified.contains(&n) {
        return true;
    }
    if !probable_prime(n) {
        return false;
    }
    if n <= u64::MAX as u128 {
        certified.insert(n);
        return true;
    }
    let mut divisors = Vec::new();
    factors(n - 1, &mut divisors, certified);
    divisors.sort_unstable();
    divisors.dedup();
    for witness in 2..n {
        if gcd(witness, n) != 1 || pow_mod(witness, n - 1, n) != 1 {
            return false;
        }
        let mut valid = true;
        for &q in &divisors {
            let power = pow_mod(witness, (n - 1) / q, n);
            let difference = if power == 0 { n - 1 } else { power - 1 };
            let divisor = gcd(difference, n);
            if divisor > 1 && divisor < n {
                return false;
            }
            if divisor == n {
                valid = false;
                break;
            }
        }
        if valid {
            certified.insert(n);
            return true;
        }
    }
    false
}

fn factors(n: u128, out: &mut Vec<u128>, certified: &mut HashSet<u128>) {
    if n < 2 {
        return;
    }
    if certified_prime(n, certified) {
        out.push(n);
        return;
    }
    let d = rho(n);
    factors(d, out, certified);
    factors(n / d, out, certified);
}

fn factor_line(n: u128, exponents: bool) -> String {
    let mut fs = Vec::new();
    factors(n, &mut fs, &mut HashSet::new());
    fs.sort_unstable();
    let mut result = format!("{n}:");
    let mut i = 0;
    while i < fs.len() {
        let mut end = i + 1;
        while end < fs.len() && fs[end] == fs[i] {
            end += 1;
        }
        if exponents {
            result.push_str(&format!(" {}", fs[i]));
            if end - i > 1 {
                result.push_str(&format!("^{}", end - i));
            }
        } else {
            for _ in i..end {
                result.push_str(&format!(" {}", fs[i]));
            }
        }
        i = end;
    }
    result.push('\n');
    result
}

pub fn run(arguments: &[String]) -> i32 {
    let items = match args::parse_command(
        "factor",
        arguments,
        HELP,
        &[OptSpec::flag("exponents", 'h')],
        true,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut exponents = false;
    let mut numbers = Vec::new();
    for item in items {
        match item {
            Item::Opt("exponents", _) => exponents = true,
            Item::Operand(v) => numbers.push(v),
            _ => {}
        }
    }
    let mut failed = false;
    let mut read_failed = false;
    let status = io::output("factor", |out| {
        let mut emit = |s: &str| -> std::io::Result<()> {
            let digits = s.strip_prefix('+').unwrap_or(s);
            match digits.parse::<u128>() {
                Ok(n) if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) => {
                    out.write_all(factor_line(n, exponents).as_bytes())?;
                }
                _ => {
                    if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
                        eprintln!("factor: '{s}' is too large");
                    } else {
                        eprintln!("factor: '{s}' is not a valid positive integer");
                    }
                    failed = true;
                }
            }
            Ok(())
        };
        if numbers.is_empty() {
            let mut reader = io::input("-")?;
            let mut token = Vec::new();
            loop {
                let buf = match reader.fill_buf() {
                    Ok(buf) => buf,
                    Err(e) => {
                        eprintln!("factor: {}", crate::common::error::io_text(&e));
                        read_failed = true;
                        break;
                    }
                };
                if buf.is_empty() {
                    break;
                }
                let len = buf.len();
                for &byte in buf {
                    if byte.is_ascii_whitespace() {
                        if !token.is_empty() {
                            emit(&String::from_utf8_lossy(&token))?;
                            token.clear();
                        }
                    } else {
                        token.push(byte);
                    }
                }
                reader.consume(len);
            }
            if !token.is_empty() {
                emit(&String::from_utf8_lossy(&token))?;
            }
        } else {
            for s in &numbers {
                emit(s)?;
            }
        }
        Ok(())
    });
    if failed || read_failed {
        1
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arithmetic() {
        let n = u128::MAX;
        assert_eq!(mul_mod(n - 2, n - 3, n), 6);
        assert_eq!(pow_mod(2, 10, 1000), 24);
    }
    #[test]
    fn primality() {
        let mut cache = HashSet::new();
        for p in [2, 3, 97, 1_000_000_007] {
            assert!(certified_prime(p, &mut cache));
        }
        for n in [0, 1, 4, 561, 3215031751] {
            assert!(!certified_prime(n, &mut cache));
        }
    }
    #[test]
    fn large_primes_have_recursive_certificates() {
        let mut cache = HashSet::new();
        let p = 18_446_744_073_709_551_629;
        assert!(certified_prime(p, &mut cache));
        assert!(cache.contains(&p));
        assert!(!certified_prime(
            318_665_857_834_031_151_167_461,
            &mut cache
        ));
    }
    #[test]
    fn factoring() {
        assert_eq!(factor_line(360, false), "360: 2 2 2 3 3 5\n");
        assert_eq!(factor_line(360, true), "360: 2^3 3^2 5\n");
        assert_eq!(factor_line(1, false), "1:\n");
    }
}
