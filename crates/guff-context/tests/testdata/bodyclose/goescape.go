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

// --- Only the *first* capture decides -----------------------------------------
//
// `isopen` walks the captured cell's referrers and returns at the first
// `*ssa.MakeClosure`: a goroutine that captures the response after a closure
// that closes it is never asked. And `go goSink(resp)` with no literal reaches
// an `*ssa.Go` that no arm matches — not a hand-off, not a leak, just skipped.
// dapr's `serviceinvocation/http/sserelay.go` is the first shape.

func goSink(r *http.Response) { _ = r }

// Silent: the closing closure is deferred under `if resp != nil`, and the
// goroutine that reads the body comes after it (dapr).
func DeferredClosureBeforeGoroutine(url string) {
	resp, err := http.Get(url)
	if resp != nil {
		defer func() {
			_ = resp.Body.Close()
		}()
	}
	if err != nil {
		return
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		_, _ = bufio.NewReader(resp.Body).ReadString('\n')
	}()
	<-done
}

// Silent: the same without the `if`.
func DeferredClosureThenGoroutineReadsBody(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	defer func() { _ = resp.Body.Close() }()
	go func() { _ = bufio.NewReader(resp.Body) }()
}

// Silent: the goroutine only reads a field.
func DeferredClosureThenGoroutineReadsField(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	defer func() { _ = resp.Body.Close() }()
	go func() { _ = resp.StatusCode }()
}

// Reported: the goroutine is the first capture; the closing closure after it
// is never asked.
func GoroutineBeforeDeferredClosure(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	go func() { _ = resp.StatusCode }()
	defer func() { _ = resp.Body.Close() }()
}

// Silent: `go goSink(resp)` is skipped and the deferred close settles it.
func GoSinkWithDeferredClose(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	defer resp.Body.Close()
	go goSink(resp)
}

// Reported: `go goSink(resp)` is skipped, and nothing else closes the body.
func GoSinkAlone(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	go goSink(resp)
}

// Silent: a closing deferred closure, then `go goSink(resp)`.
func DeferredClosureThenGoSink(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	defer func() { _ = resp.Body.Close() }()
	go goSink(resp)
}

// Silent: `go goSink(resp)` first — it is not a capture — then the closing
// deferred closure, which is.
func GoSinkThenDeferredClosure(url string) {
	resp, err := http.Get(url)
	if err != nil {
		return
	}
	go goSink(resp)
	defer func() { _ = resp.Body.Close() }()
}
