//! Default symbols excluded from errcheck (kisielk/errcheck parity).

/// Symbols that errcheck skips by default (fmt.Print to buffers, hash.Write, etc.).
pub const DEFAULT_EXCLUDED_SYMBOLS: &[&str] = &[
    // bytes
    "(*bytes.Buffer).Write",
    "(*bytes.Buffer).WriteByte",
    "(*bytes.Buffer).WriteRune",
    "(*bytes.Buffer).WriteString",
    // crypto
    "crypto/rand.Read",
    // fmt
    "fmt.Print",
    "fmt.Printf",
    "fmt.Println",
    "fmt.Fprint(*bytes.Buffer)",
    "fmt.Fprintf(*bytes.Buffer)",
    "fmt.Fprintln(*bytes.Buffer)",
    "fmt.Fprint(*strings.Builder)",
    "fmt.Fprintf(*strings.Builder)",
    "fmt.Fprintln(*strings.Builder)",
    "fmt.Fprint(os.Stderr)",
    "fmt.Fprintf(os.Stderr)",
    "fmt.Fprintln(os.Stderr)",
    // io
    "(*io.PipeReader).CloseWithError",
    "(*io.PipeWriter).CloseWithError",
    // math/rand
    "math/rand.Read",
    "(*math/rand.Rand).Read",
    // strings
    "(*strings.Builder).Write",
    "(*strings.Builder).WriteByte",
    "(*strings.Builder).WriteRune",
    "(*strings.Builder).WriteString",
    // hash
    "(hash.Hash).Write",
    "(*crypto/sha3.SHA3).Write",
    "(*crypto/sha3.SHAKE).Read",
    "(*crypto/sha3.SHAKE).Write",
    // hash/maphash
    "(*hash/maphash.Hash).Write",
    "(*hash/maphash.Hash).WriteByte",
    "(*hash/maphash.Hash).WriteString",
];
