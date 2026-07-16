package gocritic

import (
	"fmt"
	formatting "fmt"
	"path/filepath"
	"regexp"
	"sort"
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
