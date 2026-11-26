package p

import (
	"fmt"
	"math"
	"net/http"
	"strings"
	"time"
)

// S1003
func BadIndex(s string) bool {
	return strings.Index(s, "x") == -1
}

// SA4017 + S1039
func DiscardedSprintf() {
	fmt.Sprintf("unused result")
}

func UsedSprintf() string {
	return fmt.Sprintf("hello")
}

// S1009: should omit nil check before len
func NilLen(s []int) bool {
	return s != nil && len(s) != 0
}

// SA4023: typed nil stored in interface, then compared to nil
func TypedNilIface() bool {
	var p *int
	var i any
	i = p
	return i == nil
}

// math.MaxInt64 is an untyped constant defaulting to int, so the int64 is
// load-bearing and QF1011 must not suggest removing it.
func UntypedConstDecl() int64 {
	var ts int64 = math.MaxInt64
	return ts
}

// Here the default type of the constant is the declared type.
func UntypedConstMatchingDefault() int {
	var i int = math.MaxInt32
	return i
}

// time.Nanosecond is already typed, so the declared type really is redundant.
func TypedQualifiedConst() time.Duration {
	var d time.Duration = time.Nanosecond
	return d
}

func DeclareThenAssign(f func() error) error {
	var err error
	err = f()
	return err
}

type meta struct {
	Labels map[string]string
}

type record struct {
	meta
}

func EmbeddedSelector(r *record) {
	r.meta.Labels = nil
}

// resp is only dereferenced once err is known to be nil.
func NilCheckThenShortCircuitDeref() bool {
	resp, err := http.Get("http://example.com")
	if resp != nil {
		_ = resp.Status
	}
	return err == nil && resp.StatusCode == http.StatusOK
}
