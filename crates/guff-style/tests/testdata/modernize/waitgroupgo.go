//go:build go1.25

package modernize

import "sync"

func spawn(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() {
		defer wg.Done()
		_ = 1
	}()
}

// "Body must start with `defer wg.Done()` or end with `wg.Done()`." guff had
// only the first; beats' synthexec writes the second three times in a row, and
// it is the shape a goroutine that logs its own error naturally takes.

type wgHolder struct{ wg sync.WaitGroup }

func spawnTrailingDone(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() {
		work()
		wg.Done()
	}()
}

// The receiver only has to be the same *syntax* the `Add` used.
func spawnIndexedReceiver(wgs []sync.WaitGroup) {
	wgs[0].Add(1)
	go func() {
		work()
		wgs[0].Done()
	}()
}

func spawnFieldReceiver(h *wgHolder) {
	h.wg.Add(1)
	go func() {
		work()
		h.wg.Done()
	}()
}

// Silent: `Done` is neither first-and-deferred nor last.
func spawnDoneInTheMiddle(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() {
		wg.Done()
		work()
	}()
}

// Silent: not `Add(1)`.
func spawnAddTwo(wg *sync.WaitGroup) {
	wg.Add(2)
	go func() {
		work()
		wg.Done()
	}()
}

// Silent: `wg.Go` takes a `func()`.
func spawnWithResult(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() error {
		work()
		wg.Done()
		return nil
	}()
}

// Silent: the `go` statement is not the next one.
func spawnNotAdjacent(wg *sync.WaitGroup) {
	wg.Add(1)
	work()
	go func() {
		work()
		wg.Done()
	}()
}

// Silent: a different WaitGroup.
func spawnMismatched(a, b *sync.WaitGroup) {
	a.Add(1)
	go func() {
		defer b.Done()
		work()
	}()
}

func work() {}
