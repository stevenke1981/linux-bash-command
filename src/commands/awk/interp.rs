mod builtins;
mod eval;
mod input;
mod statements;
mod variables;
use super::{
    ast::*,
    fields::Fields,
    io::{Files, Records},
    regexp::Cache,
    value::{Array, Scalar, Slot},
};
use std::{
    collections::HashMap,
    io::{self, Write},
    rc::Rc,
};

#[derive(Debug)]
pub enum Error {
    Message(String),
    Io(io::Error),
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return(Scalar),
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Message(message.into())
    }
}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
struct Special {
    nr: Id,
    nf: Id,
    fnr: Id,
    fs: Id,
    ofs: Id,
    ors: Id,
    rs: Id,
    filename: Id,
    subsep: Id,
    rstart: Id,
    rlength: Id,
    convfmt: Id,
    ofmt: Id,
    environ: Id,
    argc: Id,
    argv: Id,
}
impl Special {
    fn new(program: &mut Program) -> Self {
        Self {
            nr: program.intern("NR"),
            nf: program.intern("NF"),
            fnr: program.intern("FNR"),
            fs: program.intern("FS"),
            ofs: program.intern("OFS"),
            ors: program.intern("ORS"),
            rs: program.intern("RS"),
            filename: program.intern("FILENAME"),
            subsep: program.intern("SUBSEP"),
            rstart: program.intern("RSTART"),
            rlength: program.intern("RLENGTH"),
            convfmt: program.intern("CONVFMT"),
            ofmt: program.intern("OFMT"),
            environ: program.intern("ENVIRON"),
            argc: program.intern("ARGC"),
            argv: program.intern("ARGV"),
        }
    }
}
pub struct Runtime<'a> {
    program: Rc<Program>,
    special: Special,
    globals: Vec<Slot>,
    locals: Vec<HashMap<Id, Slot>>,
    fields: Fields,
    cache: Cache,
    files: Files,
    input: Option<Records>,
    input_index: usize,
    opened_input: bool,
    stdin_done: bool,
    out: &'a mut dyn Write,
    exit_status: i32,
    phase: u8,
    seed: f64,
    random: u64,
}
impl<'a> Runtime<'a> {
    pub fn new(mut program: Program, args: &[String], out: &'a mut dyn Write) -> Self {
        let special = Special::new(&mut program);
        let count = program.names.len();
        let mut runtime = Self {
            program: Rc::new(program),
            special,
            globals: vec![Slot::Unset; count],
            locals: Vec::new(),
            fields: Fields::default(),
            cache: Cache::default(),
            files: Files::default(),
            input: None,
            input_index: 1,
            opened_input: false,
            stdin_done: false,
            out,
            exit_status: 0,
            phase: 0,
            seed: 0.0,
            random: 0x9e3779b97f4a7c15,
        };
        runtime.initialize(args);
        runtime
    }
    fn initialize(&mut self, args: &[String]) {
        for id in [
            self.special.nr,
            self.special.nf,
            self.special.fnr,
            self.special.rstart,
        ] {
            self.globals[id] = Slot::Scalar(Scalar::Number(0.0));
        }
        self.globals[self.special.rlength] = Slot::Scalar(Scalar::Number(0.0));
        for (id, bytes) in [
            (self.special.fs, b" ".as_slice()),
            (self.special.ofs, b" "),
            (self.special.ors, b"\n"),
            (self.special.rs, b"\n"),
            (self.special.filename, b""),
            (self.special.subsep, b"\x1c"),
            (self.special.convfmt, b"%.6g"),
            (self.special.ofmt, b"%.6g"),
        ] {
            self.globals[id] = Slot::Scalar(Scalar::string(bytes.to_vec()));
        }
        let mut argv = std::collections::BTreeMap::new();
        argv.insert(b"0".to_vec(), Scalar::input(b"awk".to_vec()));
        for (index, arg) in args.iter().enumerate() {
            argv.insert(
                (index + 1).to_string().into_bytes(),
                Scalar::input(arg.as_bytes().to_vec()),
            );
        }
        self.globals[self.special.argv] = Slot::Array(Rc::new(std::cell::RefCell::new(argv)));
        self.globals[self.special.argc] = Slot::Scalar(Scalar::Number((args.len() + 1) as f64));
        let mut environ = std::collections::BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            environ.insert(os_bytes(key), Scalar::input(os_bytes(value)));
        }
        self.globals[self.special.environ] = Slot::Array(Rc::new(std::cell::RefCell::new(environ)));
    }
    pub fn assign(&mut self, name: &[u8], bytes: &[u8]) -> Result<()> {
        let name = std::str::from_utf8(name).map_err(|_| Error::from("invalid variable name"))?;
        if super::analyze::builtin(name) || super::parser::reserved_name(name) {
            return Err(Error::from("reserved name used as a variable"));
        }
        let value = Scalar::input(super::io::argument(bytes)?);
        if let Some(id) = self.program.identifiers.get(name).copied() {
            if self.program.functions.contains_key(&id) {
                return Err(Error::from("function name used as a variable"));
            }
            self.set_var(id, value)?;
        }
        Ok(())
    }
    pub fn execute(&mut self) -> Result<i32> {
        let program = self.program.clone();
        let mut stopped = false;
        for action in &program.begin {
            match self.statement(action) {
                Err(Error::Exit) => {
                    stopped = true;
                    break;
                }
                result => result?,
            }
        }
        if !stopped && (!program.rules.is_empty() || !program.end.is_empty()) {
            self.phase = 1;
            let mut ranges = vec![false; program.rules.len()];
            'records: while let Some(record) = self.read_main()? {
                self.set_record(record)?;
                for (index, rule) in program.rules.iter().enumerate() {
                    match self.apply_rule(rule, &mut ranges[index]) {
                        Err(Error::Next) => continue 'records,
                        Err(Error::NextFile) => {
                            self.input = None;
                            continue 'records;
                        }
                        Err(Error::Exit) => break 'records,
                        result => result?,
                    }
                }
            }
        }
        self.phase = 2;
        for action in &program.end {
            match self.statement(action) {
                Err(Error::Exit) => break,
                result => result?,
            }
        }
        self.files.flush(None)?;
        Ok(self.exit_status)
    }
    fn selected(&mut self, pattern: &Pattern, range: &mut bool) -> Result<bool> {
        match pattern {
            Pattern::Always => Ok(true),
            Pattern::Expression(e) => Ok(self.eval(e)?.truth()),
            Pattern::Range(first, last) => {
                if !*range && self.eval(first)?.truth() {
                    *range = true;
                }
                let selected = *range;
                if *range && self.eval(last)?.truth() {
                    *range = false;
                }
                Ok(selected)
            }
        }
    }
    fn apply_rule(&mut self, rule: &Rule, range: &mut bool) -> Result<()> {
        if self.selected(&rule.pattern, range)? {
            self.statement(&rule.action)?;
        }
        Ok(())
    }
    fn set_record(&mut self, bytes: Vec<u8>) -> Result<()> {
        let fs = self.text(self.special.fs)?;
        let paragraph = self.text(self.special.rs)?.is_empty();
        self.fields.record(bytes, fs, paragraph);
        Ok(())
    }
}
fn os_bytes(value: std::ffi::OsString) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        value.into_vec()
    }
    #[cfg(not(unix))]
    {
        value.to_string_lossy().as_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(program: &str) -> Vec<u8> {
        let program = super::super::parser::parse(program.as_bytes().to_vec()).unwrap();
        let mut out = Vec::new();
        assert_eq!(Runtime::new(program, &[], &mut out).execute().unwrap(), 0);
        out
    }
    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(run("BEGIN {print -2^2,2^3^2,7%3}"), b"-4 512 1\n");
    }
    #[test]
    fn strings_and_strnum() {
        assert_eq!(
            run("BEGIN {print (\"10\"<2),(2<10),sprintf(\"%04d\",3)}"),
            b"1 1 0003\n"
        );
    }
    #[test]
    fn conditional_short_circuit() {
        assert_eq!(
            run("BEGIN {x=0;print (1||x++),(0&&x++),(x?2:3),x}"),
            b"1 0 3 0\n"
        );
    }
    #[test]
    fn loops_and_control() {
        assert_eq!(run("BEGIN {for(i=0;i<5;i++){if(i==2)continue;if(i==4)break;s+=i} do s++;while(s<5);while(s<6)s++;print s}"),b"6\n");
    }
    #[test]
    fn arrays_and_subsep() {
        assert_eq!(
            run("BEGIN {a[1,2]=3;print ((1,2) in a),a[1,2],length(a);delete a;print length(a)}"),
            b"1 3 1\n0\n"
        );
    }
    #[test]
    fn recursive_functions() {
        assert_eq!(
            run("function f(n){return n<2?1:n*f(n-1)} BEGIN {print f(6)}"),
            b"720\n"
        );
    }
    #[test]
    fn scalar_copy_and_array_reference() {
        assert_eq!(
            run("function f(a,x){a[1]=9;x=8} BEGIN {x=1;f(a,x);print a[1],x}"),
            b"9 1\n"
        );
    }
    #[test]
    fn split_and_match() {
        assert_eq!(run("BEGIN {n=split(\"a::b\",a,/:+/);print n,a[2];print match(\"zab\",/a|ab/),RSTART,RLENGTH}"),b"2 b\n2 2 2\n");
    }
    #[test]
    fn substitution_and_empty_matches() {
        assert_eq!(
            run("BEGIN {s=\"ab\";print gsub(/a*/,\"X\",s),s;sub(/b/,\"[&]\",s);print s}"),
            b"2 XbX\nX[b]X\n"
        );
    }
    #[test]
    fn strings_and_case() {
        assert_eq!(run("BEGIN {print length(\"abc\"),substr(\"abcd\",2,2),index(\"abc\",\"b\"),tolower(\"AB\"),toupper(\"ab\")}"),b"3 bc 2 ab AB\n");
    }
    #[test]
    fn math_builtins() {
        assert_eq!(
            run("BEGIN {print sin(0),cos(0),atan2(0,1),exp(0),log(1),sqrt(9),int(-2.9)}"),
            b"0 1 0 1 0 3 -2\n"
        );
    }
    #[test]
    fn numeric_formats_are_distinct() {
        assert_eq!(
            run("BEGIN {CONVFMT=\"%.2f\";OFMT=\"%.1f\";x=1.234;print x,x\"\",sprintf(\"%.3f\",x)}"),
            b"1.2 1.23 1.234\n"
        );
    }
    #[test]
    fn record_rebuild_captures_ofs_but_uses_current_convfmt() {
        assert_eq!(
            run("BEGIN{$0=\"a b\";OFS=\":\";$1=1.25;OFS=\"!\";CONVFMT=\"%.3f\";print $0}"),
            b"1.250:b\n"
        );
    }
    #[test]
    fn parenthesized_concat_is_not_a_spaced_function_call() {
        assert_eq!(run("BEGIN{f=2;print f (1)}"), b"21\n");
    }
}
