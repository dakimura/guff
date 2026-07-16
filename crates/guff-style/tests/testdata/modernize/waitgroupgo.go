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
