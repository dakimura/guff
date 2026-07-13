package ok

import "sync"

func ok(m *sync.Mutex) {
	_ = m
}
