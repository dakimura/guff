// Package g7xx is gosec's taint engine: G702 (command injection), G703 (path
// traversal), G705 (XSS), G706 (log injection) and G710 (open redirect). One
// engine, five tables of sources, sinks and sanitizers.
//
// Every function is marked `// fires` or `// silent`, and the silent ones are
// the point: a taint rule that reports everything is easy, and gosec's answer
// is decided by where a source may come from, what neutralizes it, and which
// argument of a sink is even looked at.
package g7xx

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"html"
	"html/template"
	"io"
	"log"
	"log/slog"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
)

// --- G702: command injection ------------------------------------------------

// fires — `os.Args` is a source, and every argument of `exec.Command` is
// checked (the rule names no CheckArgs).
func g702ArgsToCommand() {
	args := os.Args
	_ = exec.Command(args[0], args[1:]...)
}

// fires — `os.Getenv`.
func g702EnvToCommand() {
	_ = exec.Command("sh", "-c", os.Getenv("GUFF_G702"))
}

// fires — a *http.Request parameter of an exported bare function. It is
// tainted even though the call graph shows no caller at all, because a
// framework can register one through dispatch the graph cannot see
// (`mayHaveExternalCallers`).
func G702HandlerToCommand(w http.ResponseWriter, r *http.Request) {
	_ = exec.Command("sh", "-c", r.FormValue("cmd"))
}

// fires — `syscall.Exec` is a sink too.
func g702SyscallExec() {
	_ = syscall.Exec(os.Args[0], os.Args, os.Environ())
}

// fires — taint reaches the result of any callee gosec cannot look inside,
// which is every stdlib helper.
func g702ThroughStdlibCall() {
	parts := []string{os.Getenv("GUFF_G702")}
	_ = exec.Command("sh", "-c", strings.Join(parts, " "))
}

// fires — a struct field that was assigned from a source. The walk is
// field-sensitive: it looks for a store to *this* field, not at the struct.
func g702FieldTaint() {
	h := g702Holder{cmd: os.Getenv("GUFF_G702")}
	_ = exec.Command("sh", "-c", h.cmd)
}

type g702Holder struct {
	cmd  string
	safe string
}

// silent — the tainted field is a different one.
func g702OtherFieldIsClean() {
	h := g702Holder{cmd: os.Getenv("GUFF_G702"), safe: "ls"}
	_ = exec.Command("sh", "-c", h.safe)
}

// silent — literals only.
func g702Literal() {
	_ = exec.Command("ls", "-l")
}

// silent — SSA is not a flat "was this name ever assigned a source": the second
// assignment is a different value and it is not tainted.
func g702TaintDies() {
	s := os.Getenv("GUFF_G702")
	s = "constant"
	_ = exec.Command("sh", "-c", s)
}

// silent — the initializer that assigns this is not a source function guff or
// gosec analyses, so the global carries no taint.
var g702Global = os.Getenv("GUFF_G702")

// silent
func g702GlobalTaint() {
	_ = exec.Command("sh", "-c", g702Global)
}

// The interprocedural case: `os.Args` two calls above the sink. Nothing here is
// a source type; the taint arrives entirely through the call graph.
func g702Outer() {
	args := os.Args
	_ = g702Mid(args[0], args)
}

func g702Mid(binary string, args []string) error {
	return g702Inner(binary, args)
}

// fires
func g702Inner(binary string, args []string) error {
	return syscall.Exec(binary, args, os.Environ())
}

// --- G703: path traversal ---------------------------------------------------

// fires
func g703EnvOpen() {
	f, _ := os.Open(os.Getenv("GUFF_G703"))
	_ = f
}

// silent — `filepath.Clean` is a declared sanitizer.
func g703CleanIsSanitized() {
	f, _ := os.Open(filepath.Clean(os.Getenv("GUFF_G703")))
	_ = f
}

// silent — so is `filepath.Base`.
func g703BaseIsSanitized() {
	f, _ := os.Open(filepath.Base(os.Getenv("GUFF_G703")))
	_ = f
}

// fires — `filepath.Join` is not a sanitizer, and it has no body here, so the
// tainted argument reaches its result.
func g703JoinIsNotSanitized(dir string) {
	f, _ := os.Open(filepath.Join(dir, os.Getenv("GUFF_G703")))
	_ = f
}

// fires — `http.ServeFile`'s path argument.
func G703ServeFile(w http.ResponseWriter, r *http.Request) {
	http.ServeFile(w, r, r.URL.Path)
}

// silent — `ServeFile` names CheckArgs [2], so the tainted *http.Request in
// argument 1 is not looked at. Without the index every handler serving a fixed
// file would report.
func G703ServeFileConstPath(w http.ResponseWriter, r *http.Request) {
	http.ServeFile(w, r, "/static/index.html")
}

// fires — a *bufio.Scanner parameter is a source type.
func g703ScannerSource(s *bufio.Scanner) {
	f, _ := os.Open(s.Text())
	_ = f
}

// fires — `os.ReadFile` is a source as well as a sink, and `os.WriteFile`
// checks every argument, so the *content* fires.
func g703ReadFileContentIntoWriteFile(dir string) error {
	c := filepath.Join(dir, "cfg.json")
	raw, err := os.ReadFile(c)
	if err != nil {
		return err
	}
	return os.WriteFile(c, raw, 0o600)
}

// silent — `strconv.Atoi` is a sanitizer for a path rule: an integer cannot
// contain a separator.
func g703NumericIsSanitized() {
	n, _ := strconv.Atoi(os.Getenv("GUFF_G703"))
	_ = os.Remove(strconv.Itoa(n))
}

// --- G706: log injection ----------------------------------------------------

// fires — a field read on a source-typed *parameter* is tainted with no
// entry-point question asked at all (`isFieldAccessTainted` CASE 1).
func G706HandlerLog(w http.ResponseWriter, r *http.Request) {
	log.Println(r.URL.Path)
}

// silent — `strconv.Quote` escapes the newlines this rule is about.
func G706HandlerLogQuoted(w http.ResponseWriter, r *http.Request) {
	log.Println(strconv.Quote(r.URL.Path))
}

// silent — so does `strings.ReplaceAll`, which is a sanitizer *here* and not
// for G703.
func G706HandlerLogReplaced(w http.ResponseWriter, r *http.Request) {
	log.Println(strings.ReplaceAll(r.URL.Path, "\n", ""))
}

// fires — slog's CheckArgs is [0]: the message reaches the output verbatim.
func G706SlogMessage(w http.ResponseWriter, r *http.Request) {
	slog.Warn(r.URL.Path)
}

// silent — the attribute *values* are escaped by both handlers, so a tainted
// one is not a finding.
func G706SlogAttrValueIsSafe(w http.ResponseWriter, r *http.Request) {
	slog.Warn("static message", "path", r.URL.Path)
}

// fires — *url.URL is a source type too.
func g706URLSource(u *url.URL) {
	log.Printf("%s", u.Path)
}

// fires
func g706EnvLog() {
	log.Println(os.Getenv("GUFF_G706"))
}

// silent
func g706Literal() {
	log.Println("hello")
}

// The two halves of the call-graph rule. `plainRec` is unexported and never
// converted to an interface, so `ssautil.AllFunctions` — which is CHA's node
// set — never reaches its methods: `plainServe` is not a caller anyone can see
// and `plainLog`'s `id` stays clean.

type plainRec struct{}

// silent
func (p *plainRec) plainLog(id string) {
	log.Printf("plain %s", id)
}

func (p *plainRec) plainServe(w http.ResponseWriter, r *http.Request) {
	p.plainLog(r.URL.Path)
}

// `boxedRec` is the same code, except that it is converted to an interface
// below. That puts it in `RuntimeTypes`, so its methods join the call graph and
// the taint crosses the call.

type boxedRec struct{}

// fires
func (b *boxedRec) boxedLog(id string) {
	log.Printf("boxed %s", id)
}

func (b *boxedRec) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	b.boxedLog(r.URL.Path)
}

func boxedHandler() http.Handler { return &boxedRec{} }

// --- G710: open redirect ----------------------------------------------------

// fires
func G710Redirect(w http.ResponseWriter, r *http.Request) {
	http.Redirect(w, r, r.FormValue("next"), http.StatusFound)
}

// silent — argument 2 is a constant; the tainted request in argument 1 is not
// checked.
func G710RedirectConst(w http.ResponseWriter, r *http.Request) {
	http.Redirect(w, r, "/home", http.StatusFound)
}

// silent — `url.QueryEscape` cannot produce a host or a scheme.
func G710RedirectEscaped(w http.ResponseWriter, r *http.Request) {
	http.Redirect(w, r, "/go?to="+url.QueryEscape(r.FormValue("next")), http.StatusFound)
}

// fires — url.Values is a source type, and it is not a pointer one.
func g710ValuesSource(w http.ResponseWriter, r *http.Request, v url.Values) {
	http.Redirect(w, r, v.Get("next"), http.StatusFound)
}

// --- G705: XSS --------------------------------------------------------------
//
// The only rule here whose sinks are not all package functions. Two shapes the
// other four never needed:
//
//   - `(http.ResponseWriter).Write` is a method on an *interface*, so the call
//     is an SSA invoke with no static callee at all.
//   - the `fmt.Fprint*` family and `io.WriteString` are sinks only when they
//     write *to* an HTTP response. Without that guard this rule would fire on
//     most logging in a web server, and the tainted-ness of the text is not
//     what decides it.

// fires — the invoke shape. Nothing but the receiver scopes this sink.
func G705Write(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte(r.FormValue("q")))
}

// fires — guarded sink, and the writer really is the response.
func G705Fprintf(w http.ResponseWriter, r *http.Request) {
	fmt.Fprintf(w, "<p>%s</p>", r.FormValue("q"))
}

// fires — the rest of the family.
func G705FprintFamily(w http.ResponseWriter, r *http.Request) {
	fmt.Fprint(w, r.FormValue("q"))
	fmt.Fprintln(w, r.FormValue("q"))
}

// fires — `io.WriteString` names argument 1 only; argument 0 is the writer the
// guard already spoke for.
func G705IoWriteString(w http.ResponseWriter, r *http.Request) {
	io.WriteString(w, r.FormValue("q"))
}

// fires — the writer reaches the sink as a plain `io.Writer`. By then it has
// been widened, and asking whether *`io.Writer`* implements `ResponseWriter`
// answers no; the guard has to resolve back through the conversion to what the
// caller actually passed.
func G705ViaWriterParam(w http.ResponseWriter, r *http.Request) {
	var out io.Writer = w
	fmt.Fprintf(out, "<p>%s</p>", r.FormValue("q"))
}

// fires — url.Values is a source type here as well, and not a pointer one.
func G705ValuesSource(w http.ResponseWriter, v url.Values) {
	w.Write([]byte(v.Get("q")))
}

// silent — stderr is not an HTTP response, however tainted the text is. This is
// the case the guard exists for.
func g705FprintfStderr(r *http.Request) {
	fmt.Fprintf(os.Stderr, "<p>%s</p>", r.FormValue("q"))
}

// silent — same, for a concrete writer rather than another interface.
func g705FprintfBuffer(r *http.Request) {
	var b bytes.Buffer
	fmt.Fprintf(&b, "<p>%s</p>", r.FormValue("q"))
}

// silent — `io.WriteString` obeys the same guard.
func g705IoWriteStringStderr(r *http.Request) {
	io.WriteString(os.Stderr, r.FormValue("q"))
}

// silent — `html.EscapeString` is a sanitizer.
func g705Escaped(w http.ResponseWriter, r *http.Request) {
	fmt.Fprintf(w, "<p>%s</p>", html.EscapeString(r.FormValue("q")))
}

// silent — a number cannot carry a payload.
func g705Numeric(w http.ResponseWriter, r *http.Request) {
	n, _ := strconv.Atoi(r.FormValue("n"))
	fmt.Fprintf(w, "<p>%d</p>", n)
}

// silent — JSON is structurally safe and is not served as text/html.
func g705Json(w http.ResponseWriter, r *http.Request) {
	b, _ := json.Marshal(r.FormValue("q"))
	w.Write(b)
}

// silent — nothing tainted reaches either sink.
func g705Constant(w http.ResponseWriter) {
	w.Write([]byte("<p>hello</p>"))
	fmt.Fprintf(w, "<p>hi</p>")
}

// silent — `*url.URL` is a source for G703, G706 and G710, and **not** for
// G705. Nothing distinguishes the two cases except which table is being read,
// which is what makes a shared source list an easy mistake: this same fixture
// has `r.URL.Path` firing G703 and G706 forty lines up.
func g705URLIsNotAnXSSSource(w http.ResponseWriter, u *url.URL) {
	fmt.Fprintf(w, "<p>%s</p>", u.Path)
}

// silent — same story for `os.Getenv`, a function source of G702, G703 and
// G706 but not of G705. `os.Args`, one line apart in gosec's tables, *is* one.
func g705GetenvIsNotAnXSSSource(w http.ResponseWriter) {
	fmt.Fprintf(w, "<p>%s</p>", os.Getenv("HOME"))
}

// fires — and this is the pair to the two above: `os.Args` is on G705's list.
func G705ArgsSource(w http.ResponseWriter) {
	fmt.Fprintf(w, "<p>%s</p>", os.Args[1])
}

// silent *for G705*, and not because of a sanitizer. gosec's XSS table lists
// `html/template.HTML` (and HTMLAttr / JS / CSS) as sinks, but those are type
// **conversions**, and the taint engine only ever looks at `*ssa.Call`. They
// cannot fire upstream either. The line is not unwatched, though — the golden
// pins a **G203** here, from the AST rule that covers the same mistake — so
// this case records both what those four table entries are worth (nothing) and
// which rule actually catches the shape.
func g705TemplateHTML(r *http.Request) template.HTML {
	return template.HTML(r.FormValue("q"))
}
