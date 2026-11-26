package main

import "sync"

func main() {
	var x sync.Mutex
	x.Lock()
	x.Unlock()
}
