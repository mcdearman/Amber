//! Writes `src/Cases.mw` for insta.
//!
//! ```text
//! cargo run --release -- <package root>
//! ```
//!
//! Inputs, with what the crate makes of them: YAML written by its emitter,
//! snapshot files read back (headers of many shapes, and the legacy format),
//! snapshot contents compared and turned into inline literals, and whole
//! assertions -- run in a child process of this program, whose output and
//! the files it leaves are recorded. The library is ported by hand into
//! `src/`, and the crate's source is fingerprinted.

use insta::_macro_support::{SerializationFormat, serialize_value};
use insta::internals::{Content, TextSnapshotContents};
use insta::{Settings, Snapshot, TextSnapshotKind};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The crate version pinned in `Cargo.toml`.
const UPSTREAM_VERSION: &str = "1.48.0";

/// The fingerprint of the crate's source, which `src/` ports.
const SOURCES: u64 = 0x0add_6f73_18f3_cd83;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("child") {
        child(Path::new(&args[2]));
        return;
    }
    let root = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "../..".into()));

    let print = fingerprint(include_str!(concat!(env!("OUT_DIR"), "/sources.rs.txt")));
    if print != SOURCES {
        eprintln!(
            "error: insta is not the version src/ ports.\n\
             Compare its source in {} with the previous version, carry any change\n\
             into src/, then set SOURCES in scripts/generate/src/main.rs to\n\
             {print:#x}",
            env!("UPSTREAM_DIR")
        );
        std::process::exit(1);
    }

    let cases = cases();
    let path = root.join("src/Cases.mw");
    std::fs::write(&path, &cases).unwrap();
    eprintln!("wrote {} ({} bytes)", path.display(), cases.len());
}

/// FNV-1a: stable across builds, which `DefaultHasher` does not promise.
fn fingerprint(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

// --- encoding -----------------------------------------------------------------------

/// A number as `digits` base-64 digits, most significant first, each digit the
/// character `'0' + d`: `'0'` to `'o'`, one contiguous run of ASCII.
fn digits(out: &mut String, value: u64, digits: u32) {
    assert!(
        value < 1 << (6 * digits),
        "{value} does not fit in {digits} digits"
    );
    for k in (0..digits).rev() {
        out.push(char::from(b'0' + ((value >> (6 * k)) & 63) as u8));
    }
}

/// A string, as its length in bytes (3 digits) and then its bytes.
fn text(out: &mut String, s: &str) {
    digits(out, s.len() as u64, 3);
    out.push_str(s);
}

fn flag(out: &mut String, b: bool) {
    digits(out, u64::from(b), 1);
}

/// `text` as one Meadow string literal, broken with `\`-newline every `width`
/// characters. Printable ASCII and box drawing are written raw; a space that
/// would start a line is `\x20`, since a continuation drops leading whitespace.
fn long_literal(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / width * 4 + 2);
    out.push('"');
    for (i, c) in text.chars().enumerate() {
        let line_start = i > 0 && i % width == 0;
        if line_start {
            out.push_str("\\\n    ");
        }
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            ' ' if line_start => out.push_str("\\x20"),
            ' '..='~' | '\u{2500}'..='\u{257F}' => out.push(c),
            _ => {
                let _ = write!(out, "\\u{{{:X}}}", u32::from(c));
            }
        }
    }
    out.push('"');
    out
}

// --- inputs -------------------------------------------------------------------------

/// A small deterministic generator, so that the cases are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize]
    }
}

const SCALARS: &[&str] = &[
    "",
    "plain",
    "src/lib.rs",
    "two words",
    " leading",
    "trailing ",
    "a: b",
    "key:value",
    "#hash",
    "x # y",
    "-dash",
    "- item",
    "?q",
    "&anchor",
    "*alias",
    "!tag",
    "%pct",
    "@at",
    "`tick`",
    "|pipe",
    ">fold",
    "=eq",
    "yes",
    "No",
    "true",
    "False",
    "null",
    "~",
    "on",
    "y",
    "123",
    "-7",
    "+5",
    "1.5",
    "1e10",
    ".5",
    "5.",
    "inf",
    "NaN",
    "-Infinity",
    "0x1F",
    "0o17",
    ".hidden",
    "multi\nline",
    "ends\n",
    "two\n\n",
    "\n",
    "tab\there",
    "quote\"s",
    "it's",
    "back\\slash",
    "bell\u{7}",
    "del\u{7f}",
    "esc\u{1b}",
    "cr\rlf",
    "nul\u{0}",
    "unicode é 日本",
    "[list]",
    "{map}",
    "a,b",
    "foo(\"x\", 1)",
    "9223372036854775808",
    "99999999999999999999",
    "1_000",
];

/// A YAML value, serialized as insta's `Content` would be.
#[derive(Clone)]
enum Y {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Seq(Vec<Y>),
    Map(Vec<(String, Y)>),
}

impl Serialize for Y {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Y::Null => s.serialize_unit(),
            Y::Bool(b) => s.serialize_bool(*b),
            Y::Int(n) => s.serialize_i64(*n),
            Y::Str(t) => s.serialize_str(t),
            Y::Seq(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for i in items {
                    seq.serialize_element(i)?;
                }
                seq.end()
            }
            Y::Map(entries) => {
                let mut map = s.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

/// The value as `Tests.mw` rebuilds it: a kind, then its parts.
fn write_y(out: &mut String, y: &Y) {
    match y {
        Y::Null => digits(out, 0, 1),
        Y::Bool(b) => {
            digits(out, 1, 1);
            flag(out, *b);
        }
        Y::Int(n) => {
            digits(out, 2, 1);
            text(out, &n.to_string());
        }
        Y::Str(t) => {
            digits(out, 3, 1);
            text(out, t);
        }
        Y::Seq(items) => {
            digits(out, 4, 1);
            digits(out, items.len() as u64, 1);
            for i in items {
                write_y(out, i);
            }
        }
        Y::Map(entries) => {
            digits(out, 5, 1);
            digits(out, entries.len() as u64, 1);
            for (k, v) in entries {
                text(out, k);
                write_y(out, v);
            }
        }
    }
}

fn random_y(rng: &mut Rng, depth: u32) -> Y {
    match rng.below(if depth > 2 { 4 } else { 6 }) {
        0 => Y::Null,
        1 => Y::Bool(rng.chance(50)),
        2 => Y::Int(rng.next() as i64 >> rng.below(64)),
        3 => Y::Str(rng.pick(SCALARS).to_string()),
        4 => Y::Seq(
            (0..rng.below(4))
                .map(|_| random_y(rng, depth + 1))
                .collect(),
        ),
        _ => Y::Map(
            (0..rng.below(4))
                .map(|_| (rng.pick(SCALARS).to_string(), random_y(rng, depth + 1)))
                .collect(),
        ),
    }
}

fn yaml_of(y: &Y) -> String {
    serialize_value(y, SerializationFormat::Yaml)
}

fn set_block_style(on: bool) {
    // SAFETY: single-threaded; nothing else reads the environment meanwhile.
    unsafe {
        if on {
            std::env::set_var("INSTA_YAML_BLOCK_STYLE", "1");
        } else {
            std::env::remove_var("INSTA_YAML_BLOCK_STYLE");
        }
    }
}

// --- snapshot files -----------------------------------------------------------------

const HEADERS: &[&str] = &[
    "---\nsource: src/lib.rs\nexpression: x\n---\n",
    "---\nsource: src/lib.rs\nassertion_line: 12\nexpression: \"a\\nb\"\n---\n",
    "---\nsource: 'single ''quoted'''\nexpression: \"esc \\t \\u00e9 \\x41\"\n---\n",
    "---\n# a comment\nsource: src/lib.rs # trailing\ndescription: \"\"\n---\n",
    "---\nexpression: |\n  line one\n  line two\ninput_file: data.txt\n---\n",
    "---\nexpression: |-\n  kept\n\n  apart\ndescription: >\n  folded\n  text\n\n  para\n---\n",
    "---\nsource: src/lib.rs\ninfo:\n  env:\n    - A\n    - B\n  n: 3\n---\n",
    "---\nsource: src/lib.rs\ninfo: [1, two, {k: v}]\n---\n",
    "---\nsource: src/lib.rs\ninfo: {}\nassertion_line: -4\n---\n",
    "---\nsource: 42\nexpression: true\n---\n",
    "---\nsource: src/lib.rs\nsource: again.rs\n---\n",
    "---\nsource: multi\n  line plain\n  scalar\n---\n",
    "---\nexpression: \"folded\n  double\n\n  quoted\"\n---\n",
    "---\n- not\n- a map\n---\n",
    "---\n---\n",
    "---\nsource: src/lib.rs\n",
    "---  \nsource: src/lib.rs\n---   \n",
    "---\r\nsource: src/lib.rs\r\n---\r\n",
    "---\nsource: [unclosed\n---\n",
    "---\nsnapshot_kind: text\nsource: x\n---\n",
    "Created: 2019-01-01\nCreator: insta@0.1\nExpression: some + expr\nSource: src/old.rs\n\n",
    "expression: only\n\n",
    "\n",
];

const BODIES: &[&str] = &[
    "",
    "hello",
    "hello\n",
    "hello\n\n\n",
    "a\nb\nc",
    "a\r\nb\r\n",
    "  indented\n    more\n",
    "trailing   \n",
    "---\nnot a header",
    "tab\tand\u{1b}[1mesc",
];

fn snapshot_report(path: &Path) -> Vec<String> {
    match Snapshot::from_file(path) {
        Ok(s) => {
            let m = s.metadata();
            let info = match m.private_info() {
                Some(c) => serialize_content(c),
                None => "none".to_string(),
            };
            vec![
                "ok".to_string(),
                format!("name {:?} module {:?}", s.snapshot_name(), s.module_name()),
                format!("source {:?}", m.source()),
                format!("line {:?}", m.assertion_line()),
                format!("expression {:?}", m.expression()),
                format!("description {:?}", m.description()),
                format!("input {:?}", m.input_file()),
                format!("info {info}"),
                format!(
                    "contents {:?}",
                    s.as_text().map(|t| t.to_string()).unwrap_or_default()
                ),
            ]
        }
        Err(e) => vec!["error".to_string(), e.to_string()],
    }
}

fn serialize_content(c: &Content) -> String {
    serialize_value(c, SerializationFormat::Yaml)
}

// --- assertions, in a child process ---------------------------------------------------

/// One assertion: the environment, the settings, and what is on disk before.
struct Job {
    name: String,
    module: String,
    suffix: Option<String>,
    description: Option<String>,
    omit_expression: bool,
    info: Option<Y>,
    expression: String,
    line: u32,
    value: String,
    old: Option<String>,
    stale_new: bool,
    source_exists: bool,
    env: Vec<(String, String)>,
}

fn random_job(rng: &mut Rng) -> Job {
    let long_old: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let long_new: String = (1..=30)
        .map(|n| match n {
            3 => "line three\n".to_string(),
            20 => "line twenty\n".to_string(),
            _ => format!("line {n}\n"),
        })
        .collect();
    let old_bodies = [
        "hello",
        "hello\n",
        "a\nb\nc\nd\ne\nf\ng\nh\ni\nj",
        "changed",
        long_old.as_str(),
        "x\ny\nz",
    ];
    let values = [
        "hello",
        "hello  ",
        "a\nb\nC\nd\ne\nf\ng\nh\ni\nj\nk",
        "new value\nwith lines",
        "tab\there \u{1b}x",
        "",
        "crlf\r\nline",
        long_new.as_str(),
        "x\ry\nz",
        "x\ny\r\nz",
    ];
    let mut env = Vec::new();
    let update = rng.pick(&["always", "new", "no", "force", "unseen", "auto", "auto-ci"]);
    if update == "auto-ci" {
        env.push(("CI".to_string(), "true".to_string()));
    } else {
        env.push(("INSTA_UPDATE".to_string(), update.to_string()));
    }
    let output = rng.pick(&["diff", "diff", "summary", "minimal", "none"]);
    env.push(("INSTA_OUTPUT".to_string(), output.to_string()));
    if rng.chance(20) {
        env.push(("INSTA_FORCE_PASS".to_string(), "1".to_string()));
    }
    if rng.chance(15) {
        env.push(("INSTA_REQUIRE_FULL_MATCH".to_string(), "1".to_string()));
    }
    if rng.chance(20) {
        env.push(("INSTA_YAML_BLOCK_STYLE".to_string(), "1".to_string()));
    }
    if rng.chance(40) {
        env.push(("CLICOLOR_FORCE".to_string(), "1".to_string()));
    }
    let old = if rng.chance(70) {
        let body = rng.pick(&old_bodies);
        let header = if rng.chance(15) {
            rng.pick(&["Expression: legacy\n\n", "---\nsource: [broken\n---\n"])
                .to_string()
        } else {
            "---\nsource: src/lib.rs\nexpression: old\n---\n".to_string()
        };
        Some(format!("{header}{body}\n"))
    } else {
        None
    };
    Job {
        name: rng
            .pick(&["simple", "with/slash", "name.dots", "x"])
            .to_string(),
        module: rng.pick(&["", "tests", "crate::tests"]).to_string(),
        suffix: rng.chance(20).then(|| "v2".to_string()),
        description: rng
            .chance(25)
            .then(|| rng.pick(&["described", ""]).to_string()),
        omit_expression: rng.chance(20),
        info: rng.chance(20).then(|| {
            Y::Map(vec![
                ("key".to_string(), Y::Str("value".to_string())),
                ("list".to_string(), Y::Seq(vec![Y::Int(1), Y::Int(2)])),
            ])
        }),
        expression: rng.pick(&["value", "format!(\"{}\", x)"]).to_string(),
        line: rng.below(500) as u32 + 1,
        value: rng.pick(&values).to_string(),
        old,
        stale_new: rng.chance(20),
        source_exists: rng.chance(50),
        env,
    }
}

fn write_job(out: &mut String, j: &Job) {
    text(out, &j.name);
    text(out, &j.module);
    flag(out, j.suffix.is_some());
    flag(out, j.description.is_some());
    if let Some(d) = &j.description {
        text(out, d);
    }
    flag(out, j.omit_expression);
    flag(out, j.info.is_some());
    if let Some(i) = &j.info {
        write_y(out, i);
    }
    text(out, &j.expression);
    digits(out, u64::from(j.line), 2);
    text(out, &j.value);
    flag(out, j.old.is_some());
    if let Some(o) = &j.old {
        text(out, o);
    }
    flag(out, j.stale_new);
    flag(out, j.source_exists);
    digits(out, j.env.len() as u64, 1);
    for (k, v) in &j.env {
        text(out, k);
        text(out, v);
    }
}

/// The snapshot file a job's assertion uses, under `workspace`.
fn snapshot_path(workspace: &Path, j: &Job) -> PathBuf {
    let mut name = j.name.replace(['/', '\\'], "__");
    if let Some(s) = &j.suffix {
        name = format!("{}@{s}", j.name.replace(['/', '\\'], "__"));
    }
    let file = if j.module.is_empty() {
        format!("{name}.snap")
    } else {
        format!("{}__{name}.snap", j.module.replace("::", "__"))
    };
    workspace.join("src/snapshots").join(file)
}

/// Runs one job in a child process, returning what it printed and the files
/// it left.
fn run_job(workspace: &Path, j: &Job) -> Vec<String> {
    let _ = std::fs::remove_dir_all(workspace);
    std::fs::create_dir_all(workspace.join("src/snapshots")).unwrap();
    if j.source_exists {
        std::fs::write(workspace.join("src/lib.rs"), "// test\n").unwrap();
    }
    let snap = snapshot_path(workspace, j);
    if let Some(o) = &j.old {
        std::fs::write(&snap, o).unwrap();
    }
    let new_file = snap.with_extension("snap.new");
    if j.stale_new {
        std::fs::write(&new_file, "stale").unwrap();
    }
    let spec = workspace.join("job.txt");
    let mut s = String::new();
    write_job(&mut s, j);
    std::fs::write(&spec, s).unwrap();

    let (reader, writer) = std::io::pipe().unwrap();
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.arg("child")
        .arg(workspace)
        .env_clear()
        .env("PATH", "")
        .env("INSTA_WORKSPACE_ROOT", workspace)
        .stdin(Stdio::null())
        .stdout(writer.try_clone().unwrap())
        .stderr(writer);
    for (k, v) in &j.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    let mut output = String::new();
    std::io::Read::read_to_string(&mut &reader, &mut output).unwrap();
    child.wait().unwrap();

    let ws = workspace.to_string_lossy().to_string();
    let read = |p: &Path| std::fs::read_to_string(p).ok();
    vec![
        output.replace(&ws, "<WS>"),
        read(&snap).unwrap_or_else(|| "<none>".to_string()),
        read(&new_file).unwrap_or_else(|| "<none>".to_string()),
    ]
}

/// A job's spec, read back in the child.
struct SpecReader<'a>(&'a str);

impl SpecReader<'_> {
    fn num(&mut self, n: usize) -> u64 {
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        head.bytes()
            .fold(0, |acc, b| acc * 64 + u64::from(b - b'0'))
    }
    fn text(&mut self) -> String {
        let len = self.num(3) as usize;
        let (head, tail) = self.0.split_at(len);
        self.0 = tail;
        head.to_string()
    }
    fn flag(&mut self) -> bool {
        self.num(1) == 1
    }
    fn y(&mut self) -> Y {
        match self.num(1) {
            0 => Y::Null,
            1 => Y::Bool(self.flag()),
            2 => Y::Int(self.text().parse().unwrap()),
            3 => Y::Str(self.text()),
            4 => {
                let n = self.num(1);
                Y::Seq((0..n).map(|_| self.y()).collect())
            }
            _ => {
                let n = self.num(1);
                Y::Map((0..n).map(|_| (self.text(), self.y())).collect())
            }
        }
    }
}

fn child(workspace: &Path) {
    let spec = std::fs::read_to_string(workspace.join("job.txt")).unwrap();
    let mut r = SpecReader(&spec);
    let name = r.text();
    let module = r.text();
    let suffix = r.flag();
    let description = r.flag().then(|| r.text());
    let omit_expression = r.flag();
    let info = r.flag().then(|| r.y());
    let expression = r.text();
    let line = r.num(2) as u32;
    let value = r.text();

    let mut settings = Settings::clone_current();
    settings.set_prepend_module_to_snapshot(!module.is_empty());
    if suffix {
        settings.set_snapshot_suffix("v2");
    }
    if let Some(d) = description {
        settings.set_description(d);
    }
    settings.set_omit_expression(omit_expression);
    if let Some(i) = info {
        settings.set_raw_info(&serialize_to_content(&i));
    }
    let module_path = if module.is_empty() {
        "m".to_string()
    } else {
        module
    };
    // The panic is reported below; the default hook would print where it was.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        settings.bind(|| {
            insta::_macro_support::assert_snapshot(
                (Some(name.as_str()), value.as_str()).into(),
                workspace,
                "tests::f",
                &module_path,
                "src/lib.rs",
                line,
                &expression,
            )
            .unwrap();
        })
    }));
    if let Err(e) = result {
        let msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        println!("@@PANIC {msg}");
    }
}

fn serialize_to_content(y: &Y) -> Content {
    match y {
        Y::Null => Content::None,
        Y::Bool(b) => Content::Bool(*b),
        Y::Int(n) => Content::I64(*n),
        Y::Str(s) => Content::String(s.clone()),
        Y::Seq(items) => Content::Seq(items.iter().map(serialize_to_content).collect()),
        Y::Map(entries) => Content::Map(
            entries
                .iter()
                .map(|(k, v)| (Content::String(k.clone()), serialize_to_content(v)))
                .collect(),
        ),
    }
}

// --- cases --------------------------------------------------------------------------

const TEXTS: &[&str] = &[
    "",
    "one",
    "one\n",
    "  one  ",
    "\none\ntwo\n",
    "\n    a\n    b\n",
    "\n    a\n  b\n    ",
    "\n\t\ta\n\t\tb\n",
    "a\nb",
    "a\r\nb",
    "a\rb",
    "\n\r foo",
    "hello\r\n",
    "\r\nhello",
    "a\"b",
    "a\\b",
    "a\n\"#b",
    "x \"## y",
    "a\u{0}b",
    "a\tb",
    "a\t\nb",
    "\u{1b}[31m",
    "\n    ⋮line one\n    ⋮line two\n",
    "⋮a\n⋮b",
    "  ⋮x\n  y",
    "---\nbody",
    "\n\n---\nbody",
    "trailing \n",
    "é日\n  本",
];

fn cases() -> String {
    let mut rng = Rng(0x1257_a5a4_c0ff_ee42);
    let mut body = String::new();
    let mut counts = [0usize; 4];

    // Kind 0: YAML as the emitter writes it, plain and in literal blocks.
    for _ in 0..500 {
        counts[0] += 1;
        let y = if rng.chance(70) {
            Y::Map(
                (0..rng.below(5) + 1)
                    .map(|_| (rng.pick(SCALARS).to_string(), random_y(&mut rng, 0)))
                    .collect(),
            )
        } else {
            random_y(&mut rng, 0)
        };
        digits(&mut body, 0, 1);
        write_y(&mut body, &y);
        set_block_style(false);
        text(&mut body, &yaml_of(&y));
        set_block_style(true);
        text(&mut body, &yaml_of(&y));
        set_block_style(false);
    }

    // Kind 1: snapshot files read back.
    let dir = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("insta-gen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut files: Vec<(String, String)> = Vec::new();
    for h in HEADERS {
        for b in BODIES {
            files.push((h.to_string(), b.to_string()));
        }
    }
    for _ in 0..200 {
        let y = Y::Map(
            (0..rng.below(4) + 1)
                .map(|_| {
                    let key = rng.pick(&[
                        "source",
                        "expression",
                        "description",
                        "input_file",
                        "info",
                        "assertion_line",
                        "other",
                    ]);
                    (key.to_string(), random_y(&mut rng, 1))
                })
                .collect(),
        );
        set_block_style(rng.chance(40));
        let header = format!("---\n{}---\n", yaml_of(&y));
        set_block_style(false);
        files.push((header, rng.pick(BODIES).to_string()));
    }
    for (k, (header, contents)) in files.iter().enumerate() {
        counts[1] += 1;
        let file_name = rng.pick(&[
            "mod__name.snap",
            "a__b__c.snap",
            "plain.snap",
            "go1.20.5.snap",
        ]);
        let path = dir.join(format!("{k}")).join(file_name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file_text = format!("{header}{contents}");
        std::fs::write(&path, &file_text).unwrap();
        digits(&mut body, 1, 1);
        text(&mut body, file_name);
        text(&mut body, &file_text);
        let report = snapshot_report(&path);
        digits(&mut body, report.len() as u64, 1);
        for line in report {
            text(&mut body, &line);
        }
    }

    // Kind 2: contents compared, and written as inline literals.
    for _ in 0..600 {
        counts[2] += 1;
        let a = rng.pick(TEXTS);
        let b = if rng.chance(30) { a } else { rng.pick(TEXTS) };
        let (ka, kb) = (rng.chance(50), rng.chance(50));
        let indent = rng.pick(&["", "    ", "\t"]);
        let kind = |inline: bool| {
            if inline {
                TextSnapshotKind::Inline
            } else {
                TextSnapshotKind::File
            }
        };
        let ca = TextSnapshotContents::new(a.to_string(), kind(ka));
        let cb = TextSnapshotContents::new(b.to_string(), kind(kb));
        digits(&mut body, 2, 1);
        text(&mut body, a);
        text(&mut body, b);
        flag(&mut body, ka);
        flag(&mut body, kb);
        text(&mut body, indent);
        text(&mut body, &ca.to_string());
        flag(&mut body, ca.matches_latest(&cb));
        flag(&mut body, ca.matches_legacy(&cb));
        flag(&mut body, ca.matches_fully(&cb));
        text(&mut body, &ca.to_inline(indent));
    }

    // Kind 3: whole assertions.
    for k in 0..400 {
        counts[3] += 1;
        let job = random_job(&mut rng);
        let workspace = dir.join(format!("job{k}"));
        let result = run_job(&workspace, &job);
        digits(&mut body, 3, 1);
        write_job(&mut body, &job);
        for r in result {
            text(&mut body, &r);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);

    let mut out = String::new();
    let _ = writeln!(
        out,
        "-- GENERATED by scripts/generate.sh from insta {UPSTREAM_VERSION}.
-- Do not edit: run the script again instead.
--
-- Inputs, with what the crate makes of them, for `Tests.mw`: {} values
-- written as YAML, {} snapshot files read, {} pairs of snapshot contents and
-- {} assertions.
--
-- Copyright Armin Ronacher and the insta contributors, and the Meadow port's
-- authors. Licensed under Apache-2.0: see LICENSE and COPYRIGHT.

-- Each case starts with its kind (1 base-64 digit), and its fields follow in
-- the order `Tests.mw` reads them. A string is its length in bytes (3 digits)
-- and then its bytes; a YAML value is a kind and its parts.
@cfg(test)
@pub(pkg) def cases =
  {}",
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        long_literal(&body, 96)
    );
    out
}
