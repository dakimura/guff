// Package g7xx is gosec's taint engine: G702 (command injection), G703 (path
// traversal), G706 (log injection) and G710 (open redirect). One engine, four
// tables of sources, sinks and sanitizers.
//
// Every function is marked `// fires` or `// silent`, and the silent ones are
// the point: a taint rule that reports everything is easy, and gosec's answer
// is decided by where a source may come from, what neutralizes it, and which
// argument of a sink is even looked at.
package g7xx

import (
	"bufio"
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
