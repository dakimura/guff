package bad

import "sync"

func bad(wg *sync.WaitGroup) {
	go func() {
		wg.Add(1)
		defer wg.Done()
	}()
}
