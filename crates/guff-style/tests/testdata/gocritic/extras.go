package gocritic

import (
	"bytes"
	"fmt"
	formatting "fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
	// "os"
)

func emptyStringExtra(s string) {
	_ = len(s) == 0
	_ = len(s) != 0
}

func emptyFallthroughExtra(i int) {
	switch i {
	case 0:
		fallthrough
	case 1:
		_ = i
	}
}

func emptyDeclExtra() {
	var ()
	const ()
	type ()
}

func octalLiteralExtra() {
	_ = 0755
}

func nilValReturnExtra(err error) error {
	if err == nil {
		return err
	}
	return nil
}

func yodaStyleExtra(p *int) {
	if nil == p {
	}
	if 10 == *p {
	}
}

func deferUnlambdaExtra() {
	defer func() { fmt.Println("hello") }()
	formatting.Println("alias")
}

func initClauseExtra() {
	if sideEffectExtra(); true {
	}
}

func sideEffectExtra() {}

func builtinShadowExtra(len int) {
	_ = len
}

func paramCombineExtra(a int, b int) {}

func filepathJoinExtra(name string) {
	_ = filepath.Join("dir/", name)
}

func rangeAppendExtra(ns []int) {
	var rs []int
	for _, n := range ns {
		_ = n
		rs = append(rs, ns...)
	}
	_ = rs
}

func weakCondExtra(xs []int) {
	_ = xs != nil && xs[0] != 0
}

func complex64() {}

func withWidth(w int) func(*int) {
	return func(*int) {}
}

func withHeight(h int) func(*int) {
	return func(*int) {}
}

func doPanel(name string, opts ...func(*int)) {
	_ = name
	_ = opts
}

func dupOptionExtra(w, h int) {
	doPanel("hello",
		withWidth(w),
		withHeight(h),
		withWidth(w),
	)
}

type methodFoo struct{}

func (f methodFoo) bar(i int) {}

func methodExprCallExtra() {
	f := methodFoo{}
	methodFoo.bar(f, 20)
}

func rangeExprCopyExtra() {
	var xs [512]byte
	for _, x := range xs {
		_ = x
	}
}

func regexpPatternExtra() {
	regexp.MustCompile(`google.com`)
}

func badRegexpExtra() {
	regexp.MustCompile(`(?:^aa|bb|cc)foo[aba]`)
	regexp.MustCompile(`[\w_]`)
}

func regexpSimplifyExtra() {
	regexp.MustCompile(`[0-9]+`)
	regexp.MustCompile(`(?:a|b|c)`)
	regexp.MustCompile(`foo|fo`)
	regexp.MustCompile(`axx*y`)
	// Upstream names thirteen characters it will unescape and writes every
	// other escape back unchanged. A space is not one of them, in a character
	// class or out of it (argo-cd `util/sourceintegrity/gpg.go`), and an
	// escaped space does not combine with a plain one either — `canCombine`
	// starts by comparing the two operators.
	regexp.MustCompile(`^gpg: Signature made ([a-zA-Z0-9\ :]+)$`)
	regexp.MustCompile(`a\ b`)
	regexp.MustCompile(`a\  b`)
	// These thirteen are the list, so they lose the backslash.
	regexp.MustCompile(`a\&\#\!\@\%b`)
	regexp.MustCompile(`a\<\>\:\;\/\,\=b`)
}

func sortSliceExtra() {
	var xs []int
	var ys []int
	sort.Slice(xs, func(i, j int) bool {
		return ys[i] < ys[j]
	})
}

type Rows struct{}

type Result struct{}

type sqlDB struct{}

func (db *sqlDB) Query(query string, args ...interface{}) (*Rows, error) {
	return nil, nil
}

func (db *sqlDB) Exec(query string, args ...interface{}) (Result, error) {
	return Result{}, nil
}

type sqlQueryer interface {
	Query(query string, args ...interface{}) (*Rows, error)
}

func sqlQueryExtra(db *sqlDB, q sqlQueryer) {
	var err error
	_, err = db.Query("UPDATE users SET name = 'gopher'")
	_, err = q.Query("UPDATE users SET name = 'gopher'")
	_ = err
}

func typeAssertChainExtra(x interface{}) {
	if v, ok := x.(int8); ok {
		_ = v
	} else if v, ok := x.(int16); ok {
		_ = v
	}
}

func truncateCmpExtra(x int8, y int16) {
	_ = int8(y) < x
}

func (r typeDefFirstRecv) MethodBefore() {}

type typeDefFirstRecv struct{}

func deferInLoopExtra() {
	for {
		defer func() {}()
		break
	}
}

func hexLiteralExtra() {
	_ = 0X12
	_ = 0xfF
}

func nestingReduceExtra(a []int) {
	for _, v := range a {
		if v == 5 {
			_ = v
			_ = v
			_ = v
			_ = v
			_ = v
		}
	}
}

func todoDetailExtra() {
	// TODO
	_ = 0
}

// DocStubExtra ...
func DocStubExtra() {}

func unnecessaryBlockExtra() {
	a := 1
	{
		_ = a
	}
}

func sloppyReassignExtra() error {
	var err error
	if err = returnsErrorExtra(); err != nil {
		return err
	}
	return nil
}

func returnsErrorExtra() error { return nil }

func httpNoBodyExtra() {
	_, _ = http.NewRequest("GET", "http://example.com", nil)
	_, _ = http.NewRequestWithContext(nil, "GET", "http://example.com", nil)
}

func preferDecodeRuneExtra(s string) {
	_ = []rune(s)[0]
}

type byteWriterExtra struct{}

func (*byteWriterExtra) WriteRune(r rune) (int, error) { return 0, nil }
func (*byteWriterExtra) WriteByte(b byte) error         { return nil }

type runeWriterExtra struct{}

func (*runeWriterExtra) WriteRune(r rune) (int, error) { return 0, nil }

type wrongByteWriterExtra struct{}

func (*wrongByteWriterExtra) WriteRune(r rune) (int, error) { return 0, nil }
func (*wrongByteWriterExtra) WriteByte(s string) error       { return nil }

func preferWriteByteExtra(w *byteWriterExtra, runeOnly *runeWriterExtra, wrong *wrongByteWriterExtra) {
	_, _ = w.WriteRune('\n')
	_, _ = w.WriteRune('é')
	_, _ = runeOnly.WriteRune('\n')
	_, _ = wrong.WriteRune('\n')
}

func indexAllocExtra(b []byte, y string) {
	_ = strings.Index(string(b), y)
}

func stringXbytesExtra(b []byte, s string, dst []byte) {
	_ = copy(dst, []byte(s))
	_ = string(b) == ""
	_ = len(string(b))
}

func preferFilepathJoinExtra(x, y string) {
	_ = x + string(os.PathSeparator) + y
}

func stringsCompareExtra(a, b string) {
	_ = strings.Compare(a, b) == 0
	_ = strings.Compare(a, b) < 0
}

func zeroByteRepeatExtra(n int) {
	_ = bytes.Repeat([]byte{0}, n)
}

func badSortingExtra(xs []string) {
	xs = sort.StringSlice(xs)
}

func sliceClearExtra(buf []byte) {
	for i := 0; i < len(buf); i++ {
		buf[i] = 0
	}
}

type preferWriterExtra struct{}

func (*preferWriterExtra) Write(p []byte) (int, error)         { return 0, nil }
func (*preferWriterExtra) WriteString(s string) (int, error) { return 0, nil }

func preferFprintExtra(w *preferWriterExtra, x int) {
	_, _ = w.Write([]byte(fmt.Sprint(x)))
	_, _ = w.WriteString(fmt.Sprintf("%d", x))
	_, _ = io.WriteString(w, fmt.Sprintln(x))
}

func preferStringWriterExtra(w *preferWriterExtra, s string) {
	_, _ = w.Write([]byte(s))
	_, _ = io.WriteString(w, s)
}

// `m["w"].Type.Implements(…)` is `types.Implements`, which asks for the method
// set of the type *as written*. `WriteString` and `String` here have pointer
// receivers, so the value type does not implement the interface however
// addressable the expression is — and none of `preferStringWriter`,
// `preferFprint` or `redundantSprint` fires on the value forms below.
// fiber's `testConn` holds `r bytes.Buffer` by value and drew one of each.

type ptrWriterExtra struct{}

func (*ptrWriterExtra) Write(p []byte) (int, error)       { return 0, nil }
func (*ptrWriterExtra) WriteString(s string) (int, error) { return 0, nil }

type ptrStringerExtra struct{}

func (*ptrStringerExtra) String() string { return "" }

type holdsWriterExtra struct {
	byValue ptrWriterExtra
	byPtr   *ptrWriterExtra
}

// Silent: the method set of the value type has neither method.
func valueWriterExtra(v ptrWriterExtra, h *holdsWriterExtra, p ptrStringerExtra) {
	_, _ = v.Write([]byte("x"))
	_, _ = v.Write([]byte(fmt.Sprint(1)))
	_, _ = h.byValue.Write([]byte("x"))
	_, _ = h.byValue.Write([]byte(fmt.Sprint(1)))
	_ = fmt.Sprint(p)
}

// Reported: the pointer's method set has them.
func pointerWriterExtra(v *ptrWriterExtra, h *holdsWriterExtra, p *ptrStringerExtra) {
	_, _ = v.Write([]byte("x"))
	_, _ = h.byPtr.Write([]byte(fmt.Sprint(1)))
	_ = fmt.Sprint(p)
}

func syncMapLoadAndDeleteExtra(m *sync.Map, k string) {
	_, ok := m.Load(k)
	if ok {
		m.Delete(k)
	}
}

func dynamicFmtStringExtra(msg string, f func() string) error {
	_ = fmt.Errorf(msg)
	_ = fmt.Errorf(f())
	_ = fmt.Errorf("ok")
	return nil
}

func stringConcatSimplifyExtra(x, y, z, glue string) {
	_ = strings.Join([]string{x, y}, "")
	_ = strings.Join([]string{x, y, z}, "")
	_ = strings.Join([]string{x, y}, glue)
}

func badSyncOnceFuncExtra(f func()) {
	sync.OnceFunc(f)
	sync.OnceFunc(f)()
}

type protocolName string

func equalFoldExtra(x, y string, xb, yb []byte, p protocolName) {
	_ = strings.ToLower(x) == y
	_ = strings.ToUpper(x) != y
	_ = bytes.Equal(bytes.ToLower(xb), yb)
	_ = strings.ToLower(x) == x
	// `.Where(m["x"].Pure && m["y"].Pure)`: ruleguard's `isPure` accepts a type
	// conversion, and dapr's `pkg/config/configuration.go` compares two of them.
	_ = strings.ToLower(string(p)) == string(y)
}

func sprintfQuotedExtra(s string) {
	_ = fmt.Sprintf(`"%s"`, s)
	_ = fmt.Sprintf("foo `%s` bar", s)
	_ = fmt.Sprintf("%s", s)
}

func timeExprSimplifyExtra(t time.Time, tp *time.Time) {
	_ = t.Unix() / 1000
	_ = tp.UnixNano() * 1000
}

func appendCombineExtra() {
	var xs []int
	xs = append(xs, 1)
	xs = append(xs, 2)
	_ = xs
}

func unnecessaryDeferExtra() {
	defer appendCombineExtra()
	return
}

type withStringerExtra struct{}

func (withStringerExtra) String() string { return "" }

func redundantSprintExtra(s string, w withStringerExtra) {
	_ = fmt.Sprint(s)
	_ = fmt.Sprintf("%s", w)
	_ = fmt.Sprint(w)
}

type typeUnparenExtra [](func())

func importShadowExtra() {
	filepath := "x.txt"
	_ = filepath
}

// astwalk.localDefWalker decides importShadow's reach, and three shapes k9s
// contains are *not* definitions to it. Each pair below is one reported case
// next to the unreported one it is easily confused with.
func importShadowNamedResultExtra() (sort int) { return 0 }

func importShadowRangeExtra(m map[string]int) {
	// A RangeStmt is not an AssignStmt: neither name is visited.
	for sort, time := range m {
		_, _ = sort, time
	}
}

func importShadowAssignExtra() {
	// Non-define assign: the walker returns false, so the closure is skipped.
	var f func()
	f = func() {
		sort := 1
		_ = sort
	}
	f()
	// Same for a GenDecl initializer.
	var g = func() {
		time := 1
		_ = time
	}
	g()
	// Reached any other way, the closure *is* walked.
	func() {
		sort := 2
		_ = sort
	}()
}

func unnamedResultExtra() (float64, float64) {
	return 0, 0
}

func unnamedResultOkExtra() (float64, error) {
	return 0, nil
}

// unnamedResult's `typeName` reads a *types.Type*, not the syntax: every
// unnamed type — `bool`, `int`, `string`, `[]string`, `map`, `chan`, a func
// type — answers the empty string, and two empty strings are "the same name".
// Reading the name off the syntax made `bool` and `[]string` look different and
// silenced the first four shapes below; fiber carries ten
// `//nolint:gocritic // unnamedResult` directives on exactly them.
//
// Twenty shapes, measured one at a time against golangci-lint 2.12.2.

type urFoo struct{}

type urBar struct{}

// Reported.
func urBoolSlice() (bool, []string) { return false, nil }

func urBytesBoolError() ([]byte, bool, error) { return nil, false, nil }

func urIntString() (int, string) { return 0, "" }

func urIntStringError() (int, string, error) { return 0, "", nil }

func urIntInt() (int, int) { return 0, 0 }

func urFooFoo() (urFoo, urFoo) { return urFoo{}, urFoo{} }

func urSliceSlice() ([]string, []string) { return nil, nil }

func urMapMap() (map[string]int, map[string]int) { return nil, nil }

func urChanChan() (chan int, chan int) { return nil, nil }

func urNamedSliceSlice() ([]urFoo, []urFoo) { return nil, nil }

// Silent: the second name differs and is not empty, or the trailing result is
// an `error` / `bool` the checker allows.
func urIntError() (int, error) { return 0, nil }

func urFooError() (urFoo, error) { return urFoo{}, nil }

func urPtrFooError() (*urFoo, error) { return nil, nil }

func urStringBool() (string, bool) { return "", false }

func urFooBool() (urFoo, bool) { return urFoo{}, false }

func urIntBool() (int, bool) { return 0, false }

func urPtrFooPtrBar() (*urFoo, *urBar) { return nil, nil }

func urFooBarError() (urFoo, urBar, error) { return urFoo{}, urBar{}, nil }

func urNamedResults() (a, b int) { return 0, 0 }

//nolint
func whyNoLintExtra() {}

//nolint:gocritic // has explanation
func whyNoLintOkExtra() {}

func hugeParamExtra(x [100]int) {
	_ = x
}

func rangeValCopyExtra(xs [][200]byte) {
	for _, x := range xs {
		_ = x
	}
}

func ptrToRefParamExtra(m *map[string]int, ch *chan int) {}

func tooManyResultsExtra() (int, int, int, int, int, int) {
	return 0, 0, 0, 0, 0, 0
}

func evalOrderMutate(x *int) int {
	*x++
	return *x
}

func evalOrderExtra() (int, int) {
	var x int
	return x, evalOrderMutate(&x)
}

func unlabelStmtExtra(xs []int) {
label1:
	for range xs {
		break label1
	}
outer:
	for range xs {
		for range xs {
			continue outer
		}
	}
}

func returnAfterHttpErrorExtra(w http.ResponseWriter, err error) {
	if err != nil {
		http.Error(w, "err", 503)
	}
}

func commentedOutCodeExtra() {
	// fmt.Println("debugging hard")
	fmt.Println("live")
	// e.g. fmt.Println("documentation example")
}

type ExposedMutexExtra struct {
	sync.Mutex
	Port int
}

type ExposedRWMutexExtra struct {
	*sync.RWMutex
}

func badLockExtra(mu *sync.Mutex, rw *sync.RWMutex, op func()) {
	mu.Lock()
	mu.Unlock()
	op()

	rw.RLock()
	rw.RUnlock()
	op()

	rw.Lock()
	defer rw.RUnlock()
	op()

	rw.RLock()
	defer rw.Unlock()
	op()

	rw.Lock()
	defer rw.Lock()
	op()

	rw.RLock()
	defer rw.RLock()
	op()
}

func externalErrorReassignExtra() {
	io.EOF = nil
}

func uncheckedInlineErrExtra() {
	var err2 error
	if err := returnsErrorExtra(); err2 != nil {
		_ = err
	}
}

func boolExprSimplifyExtra(x, y bool, a, b int) {
	_ = !!x
	_ = !(a >= b)
	_ = !x == !y
	_ = a > b || a == b
	// removeIncDec
	_ = a < b+1
	_ = a+1 > b
	_ = a >= b+1
	_ = !(a >= b+1)
	// foldRanges
	_ = a > 10 && a < 12
	_ = a < 11 || a > 11
	// An `if`/`for` condition that is exactly one comparison keeps the type
	// `untyped bool`, and `typep.HasBoolKind` wants kind `Bool` exactly — so
	// neither of the next two is reported. The third one is: the typed operand
	// `x` types the whole condition. k9s hits this shape.
	if a+1 > b {
		_ = a
	}
	for a > b-1 {
		break
	}
	if a+1 > b && x {
		_ = a
	}
}

func unlambdaExtra() {
	_ = func(s string) string { return strings.TrimSpace(s) }
}

// badCall's rules compile to gogrep's opNonVariadicCallExpr, so a call that
// spreads a slice never matches however few arguments it has written out.
func filepathJoinVariadicExtra(elems []string) string {
	return filepath.Join(elems...)
}

type mapKeyNamedString string

type mapKeyStruct struct{ N int }

// mapKey gates on the literal's type, not on `map[K]V` syntax.
func mapKeyExtras(a *mapKeyStruct, ch chan string) {
	// Pointer keys: a repeat here is the compiler's problem, not mapKey's.
	_ = map[*mapKeyStruct]int{
		a: 1,
		a: 2,
	}
	// A named string key still has string kind underneath.
	_ = map[mapKeyNamedString]int{
		mapKeyNamedString(mapKeyIdent): 1,
		mapKeyNamedString(mapKeyIdent): 2,
	}
	// Nested literal with an elided type: no type expression to read at all.
	_ = map[string]map[string]int{
		"outer": {
			mapKeyIdent: 1,
			mapKeyIdent: 2,
		},
	}
	// Keys with side effects are skipped: two receives are not a duplicate.
	_ = map[string]int{
		<-ch: 1,
		<-ch: 2,
	}
	// The whitespace walk gives up on this literal (two spaced keys), which
	// must not take the duplicate walk down with it.
	_ = map[string]int{
		" lead":                1,
		"trail ":               2,
		mapKeyIdent:            3,
		mapKeyIdent:            4,
		string(mapKeyIdent[0]): 5,
	}
}

var mapKeyIdent = "k"

// stringXbytes carries `Where(m["b"].Type.Is("[]byte"))` on three of its rules,
// and `Type.Is` is `types.Identical` against `[]byte` — a named type over a byte
// slice is out, and a named *string* type is very much out. jaeger compares
// `attribute.Key` values this way in three places.
type attrKeyExtra string

type byteSliceExtra []byte

func stringXbytesTypedExtra(k, other attrKeyExtra, nb byteSliceExtra) {
	_ = len(string(k))
	_ = string(k) == ""
	_ = string(k) == string(other)
	// A named type over []byte is not `[]byte` either.
	_ = len(string(nb))
	_ = string(nb) == ""
}

func stringXbytesRealBytesExtra(b, c []byte) {
	_ = len(string(b))
	_ = string(b) == ""
	_ = string(b) == string(c)
}

// Upstream's stmt / stmtList / localDef walkers iterate `f.Decls` and descend
// only into `*ast.FuncDecl` bodies, so nothing below is visited by any of the
// 27 checkers built on them — not the if/else-if/else chain (`ifElseChain`),
// not the one-case switch (`singleCaseSwitch`), not the redundant block
// (`unnecessaryBlock`), not the `if` with an init clause (`initClause`). Every
// one of them is a finding when the same code sits in a function. argo-cd's
// `validatorsByGroup` is this shape and got an ifElseChain guff had alone.
var walkerScopedInVarInit = func(a, b int) int {
	if a > b {
		return a
	} else if a < b {
		return b
	} else {
		return 0
	}
}

var walkerScopedSwitchInVarInit = func(a int) string {
	switch a {
	case 1:
		return "one"
	}
	{
		_ = a
	}
	if v := a * 2; v > 4 {
		return "big"
	}
	return ""
}

// A block comment is handed to the comment checkers on its own, even when a
// line comment sits directly above it in the same group: `visitCommentGroups`
// splits the group at every `/*`, and `commentFormatting` returns as soon as a
// group starts with one. So the literal below is a finding for neither tool.
// dapr's `tests/apps/service_invocation` writes it.
var commentGroupSplit = []int{
	1,
	// A line comment with a space, immediately followed by a block comment.
	/*{
		Verb: "CONNECT",
	},*/
	2,
}
