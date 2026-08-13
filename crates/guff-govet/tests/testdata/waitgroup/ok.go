package ok

import "sync"

// Added before the goroutine starts — the correct form.
func before(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() { defer wg.Done() }()
}

// The stack shape upstream matches requires the `Add` to be the block's *first*
// statement; anything later is not this analyzer's finding.
func notFirst(wg *sync.WaitGroup) {
	go func() {
		defer wg.Done()
		wg.Add(1)
	}()
}

// Not inside a `go` statement at all.
func plainClosure(wg *sync.WaitGroup) {
	f := func() { wg.Add(1) }
	f()
}
