package bad

import "sync"

func bad(m sync.Mutex) {
	_ = m
}
