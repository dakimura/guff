// Package goescape is the `isClosureCalled` boundary.
//
// Once the response is captured, upstream decides inside the closure and never
// looks at the enclosing function again:
//
//	called := r.isClosureCalled(c)
//	return r.calledInFunc(f, called)
//
// and `isClosureCalled` counts only `*ssa.Call` and `*ssa.Defer` referrers of
// the `MakeClosure`. An `*ssa.Go` is neither, so `called` is false and every
// arm of `calledInFunc` ends in `!called` — the response is open whatever the
// closure does with the body, and whatever the caller does after.
//
// vitess's `examples/demo/demo.go:235` is the first shape below: a `defer
// resp.Body.Close()` and then a goroutine reading the body, which is a real
// use-after-close and guff called it handled.
//
// Every shape measured against golangci-lint 2.12.2.
package goescape

import (
	"bufio"
	"net/http"
	"testing"
)

// Reported: deferred close, body read from a goroutine.
func DeferredCloseWithGoroutine(url string) (<-chan string, error) {
	resp, err := http.Get(url)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	ch := make(chan string, 100)
	go func() {
		buffered := bufio.NewReader(resp.Body)
		for {
			s, err := buffered.ReadString('\n')
			if err != nil {
				close(ch)
				return
			}
			ch <- s
		}
	}()
	return ch, nil
}

// Reported: the close is inside the goroutine and deferred there.
func GoroutineDeferredClose(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	go func() {
		defer resp.Body.Close()
	}()
	return nil
}

// Reported: the close is inside the goroutine and not deferred. `called` is
// false either way.
func GoroutinePlainClose(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	go func() { resp.Body.Close() }()
	return nil
}

// Reported: the goroutine only reads a field, and the caller's plain close
// does not save it.
func GoroutineFieldReadWithClose(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	go func() { _ = resp.StatusCode }()
	resp.Body.Close()
	return nil
}

// Reported: a named function, not a literal — the value's referrer is the
// `*ssa.Go` and no arm of `isopen` matches it.
func sink(r *http.Response) { r.Body.Close() }

func GoNamedFunc(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	go sink(resp)
	return nil
}

// Reported: the literal is an *argument* of a `go` call, so the referrer is
// still the `*ssa.Go`.
func run(f func()) { f() }

func GoWithLiteralArgument(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	go run(func() { _ = resp.StatusCode })
	return nil
}

// Reported: the closure is returned and never called here.
func ReturnedClosure(url string) (func(), error) {
	resp, err := http.Get(url)
	if err != nil {
		return nil, err
	}
	return func() { resp.Body.Close() }, nil
}

// Silent: an immediately invoked literal is an `*ssa.Call` referrer.
func CalledLiteral(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	func() { resp.Body.Close() }()
	return nil
}

// Silent: so is a deferred one.
func DeferredLiteral(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	defer func() { resp.Body.Close() }()
	return nil
}

// Silent: a called literal that closes nothing is still `!called == false`.
// Upstream misses this one; matching it is the point.
func CalledLiteralNoClose(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	func() { _ = resp.StatusCode }()
	return nil
}

// Silent: the literal is an argument of an ordinary call, which makes that
// call a referrer of the `MakeClosure`.
func LiteralAsArgument(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	run(func() { _ = resp.StatusCode })
	return nil
}

// Silent: `t.Cleanup` is the same shape as `run` above.
func CleanupLiteral(t *testing.T, url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	t.Cleanup(func() { resp.Body.Close() })
	return nil
}

// Silent: the literal is held in a local and called through it.
func LiteralViaLocal(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	f := func() { _ = resp.StatusCode }
	f()
	return nil
}

// Silent: the goroutine never mentions the response — the body was taken out
// first, and that value is a leak of its own, reported at the `Get`.
func GoroutineOverBodyOnly(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	body := resp.Body
	go func() { body.Close() }()
	return nil
}

// Silent: the plain control — deferred close, no goroutine.
func PlainDeferredClose(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	return nil
}
