package revivetest

import "sync"

// forbidden-call-in-wg-go is gated on Go 1.25 (`IsAtLeastGoVersion(Go125)`),
// which reads the *package's* version from go.mod — a `//go:build` tag cannot
// raise it. extended_bad.go covers the rule for the Rust tests, which have no
// module and so count as new enough; this file exists so the golden tier can
// put the rule in a `go 1.25` module of its own.
func badForbiddenWgGoDone() {
	var wg sync.WaitGroup
	wg.Go(func() {
		wg.Done()
	})
}

func badForbiddenWgGoPanic() {
	var wg sync.WaitGroup
	wg.Go(func() {
		panic("boom")
	})
}

func okWgGo() {
	var wg sync.WaitGroup
	wg.Go(func() {
		_ = 1
	})
	wg.Wait()
}
