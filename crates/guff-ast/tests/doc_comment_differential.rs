//! `go/doc/comment` differential: every doc comment in GOROOT, plus mutations
//! of them, answered by both the real Go package and this crate's port.
//!
//! `#[ignore]`d because it shells out to `go` — the same convention as the
//! other tests CI runs in its "these want `go` on PATH" step.
//!
//! Why this exists on top of `doc_comment.rs`: upstream's txtar corpus is 53
//! hand-written cases, and a 1,100-line recursive-descent port has far more
//! reachable states than that. The two halves cover different risks — the
//! fixtures pin the cases upstream thought were interesting, this pins
//! agreement everywhere else, including inputs no human would write.
//!
//! The mutation seed is fixed, so a failure is reproducible and prints the
//! exact input, Go's answer, and ours.

use std::io::Write;
use std::process::{Command, Stdio};

/// How many mutated inputs to generate on top of the real corpus.
const MUTANTS: usize = 100_000;
const SEED: u64 = 0x676f_646f_6363_0001;

fn script() -> String {
    format!(
        "{}/../../scripts/doccomment-differential.go",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn goroot_src() -> Option<String> {
    let out = Command::new("go").args(["env", "GOROOT"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let root = String::from_utf8(out.stdout).ok()?.trim().to_string();
    let src = format!("{root}/src");
    std::path::Path::new(&src).is_dir().then_some(src)
}

fn hex_decode(s: &str) -> Vec<u8> {
    fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("non-hex byte {c:?} in differential record"),
        }
    }
    let b = s.as_bytes();
    assert!(b.len() % 2 == 0, "odd-length differential record");
    b.chunks(2).map(|p| (nib(p[0]) << 4) | nib(p[1])).collect()
}

fn hex_encode(v: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(v.len() * 2);
    for &b in v {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

/// splitmix64 — a fixed, self-contained PRNG so the corpus is reproducible
/// without pulling in a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Fragments chosen to steer the parser into branches real Go source rarely
/// reaches: headings, both list flavours, link definitions, doc links, the
/// quote conversions, and the non-ASCII arms of the `unicode` predicates.
const TOKENS: &[&str] = &[
    "", " ", "\t", "  ", "\n", "# ", "#\t", "#", " - ", "  - ", " 1. ", " 12) ", "•", "* ", "+ ",
    "[", "]", "[x]: https://e.com/a", "[x]", "[math]", "[math.Sin]", "[Parser.Parse]", "[Doc]",
    "``", "''", "```", "http://a.b/c", "https://x.y/(z)", "mailto://a.b", "ftp://x",
    "Deprecated:", "//", "/*", "*/", "\\", "{", "}", "italicword", "linkedword", "'s", ".", ":",
    "é", "あ", "\u{201c}", "—", "°", "§", "TODO", "A", "Z0",
];

fn mutate(rng: &mut Rng, text: &str) -> String {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    for _ in 0..=rng.below(4) {
        if lines.is_empty() {
            lines.push(String::new());
        }
        let i = rng.below(lines.len());
        let tok = TOKENS[rng.below(TOKENS.len())];
        match rng.below(6) {
            0 => lines.insert(i, tok.to_string()),
            1 => lines[i].insert_str(0, tok),
            2 => lines[i].push_str(tok),
            3 => {
                lines.remove(i);
            }
            4 => lines[i] = tok.to_string(),
            _ => {
                // Splice at a char boundary; a byte split would produce
                // invalid UTF-8 that Go and Rust would disagree about for
                // reasons that have nothing to do with this port.
                let n = lines[i].chars().count();
                let at = if n == 0 { 0 } else { rng.below(n + 1) };
                let byte = lines[i]
                    .char_indices()
                    .nth(at)
                    .map(|(b, _)| b)
                    .unwrap_or(lines[i].len());
                lines[i].insert_str(byte, tok);
            }
        }
    }
    lines.join("\n")
}

/// Runs the reference script over `inputs`, returning its answers.
fn reference(inputs: &[String]) -> Vec<String> {
    let mut child = Command::new("go")
        .args(["run", &script()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn go run");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut buf = String::new();
    for i in inputs {
        buf.push_str(&hex_encode(i.as_bytes()));
        buf.push('\n');
    }
    // The writer must run *concurrently* with the read below and be joined
    // after it. Both pipes carry tens of megabytes here, so feeding the child
    // to completion before draining its stdout deadlocks: the child blocks
    // writing into a full stdout pipe and stops reading stdin.
    let writer = std::thread::spawn(move || {
        stdin.write_all(buf.as_bytes()).expect("write inputs");
        drop(stdin); // close, so the child's scanner sees EOF
    });
    let out = child.wait_with_output().expect("wait for go run");
    writer.join().expect("writer thread");
    assert!(out.status.success(), "reference script failed");
    String::from_utf8(out.stdout)
        .expect("utf-8")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
#[ignore = "shells out to `go`"]
fn agrees_with_go_on_goroot_and_mutations() {
    let Some(src) = goroot_src() else {
        panic!("GOROOT/src not found — this test needs `go` on PATH");
    };

    let extracted = Command::new("go")
        .args(["run", &script(), "-extract", &src])
        .output()
        .expect("run extractor");
    assert!(extracted.status.success(), "extractor failed");
    // Comment text that is not valid UTF-8 is dropped rather than repaired: a
    // Rust `&str` cannot carry it, so guff never sees such a comment either,
    // and replacing the bad bytes would ask the two sides different questions.
    let corpus: Vec<String> = String::from_utf8(extracted.stdout)
        .expect("hex is ASCII")
        .lines()
        .filter_map(|l| String::from_utf8(hex_decode(l)).ok())
        .collect();
    assert!(
        corpus.len() > 10_000,
        "extracted only {} comments from {src} — the extractor is not seeing the corpus",
        corpus.len()
    );

    let mut rng = Rng(SEED);
    let mut inputs = corpus.clone();
    inputs.reserve(MUTANTS);
    for _ in 0..MUTANTS {
        let base = &corpus[rng.below(corpus.len())];
        inputs.push(mutate(&mut rng, base));
    }

    let want = reference(&inputs);
    assert_eq!(want.len(), inputs.len(), "reference dropped records");

    let parser = guff::doc::comment::Parser::default();
    let printer = guff::doc::comment::Printer;
    let mut mismatches = 0usize;
    let mut first = String::new();
    for (i, input) in inputs.iter().enumerate() {
        let got = printer.comment(&parser.parse(input));
        let want_i = String::from_utf8(hex_decode(&want[i])).expect("reference output is utf-8");
        if got != want_i {
            mismatches += 1;
            if first.is_empty() {
                first = format!(
                    "record {i}\n--- input ---\n{input:?}\n--- go ---\n{want_i:?}\n--- guff ---\n{got:?}"
                );
            }
        }
    }
    assert_eq!(
        mismatches,
        0,
        "{mismatches} of {} inputs diverge from go/doc/comment.\n\n{first}",
        inputs.len()
    );
}
