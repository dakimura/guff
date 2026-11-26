package main

import "sync"

func main() {
	var r sync.Mutex
	r.Lock()
	defer r.Lock()
}
