package main

import "sync"

func main() {
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		wg.Done()
	}()
	wg.Wait()
}
