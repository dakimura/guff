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

func equalFoldExtra(x, y string, xb, yb []byte) {
	_ = strings.ToLower(x) == y
	_ = strings.ToUpper(x) != y
	_ = bytes.Equal(bytes.ToLower(xb), yb)
	_ = strings.ToLower(x) == x
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
