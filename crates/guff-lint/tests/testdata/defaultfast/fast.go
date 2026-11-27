// Package fast is the fixture for `linters.default: fast`.
package fast

import (
	"errors"
	"fmt"
	"net/http"
	"os"
)

// Foo does things
func Foo(a int) int {
	x := 1
	x = 2
	if a > 0 {

		fmt.Println("recieve")
	}
	_, _, _ = a, a, a
	return a * 42
}

func init() {}

// Shapes only the non-fast linters report: govet printf, errcheck,
// unused, staticcheck. Under `default: fast` none of them runs.
func notFast() {
	fmt.Printf("%d\n", "s")
	os.Remove("x")
	if errors.New("e") == nil {
		return
	}
	_ = http.MethodGet
	_ = "GET"
}

func unusedHelper() {}

// Bar returns early
func Bar(n int) (r int) {
	if n > 1 {
		r = n
		return
	}
	for i := 0; i < n; i++ {
		v := i
		go func() { _ = v }()
	}
	return
}
