//go:build go1.21

package loopvar

import "sync"

// Per-file version: this file is go1.21, so no loop-iteration copies.
func old() {
	for mu := (sync.Mutex{}); ; {
		_ = &mu
		break
	}
}
