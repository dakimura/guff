package bad

import "sync"

func bad(m sync.Mutex) {
	_ = m
}

type Gen struct {
	mu sync.Mutex
}

func retByValue() Gen {
	var g Gen
	return g
}
