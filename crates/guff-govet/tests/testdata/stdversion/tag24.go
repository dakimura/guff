//go:build go1.24

// The build tag raises this file to go1.24: its go1.24 symbols are allowed,
// a go1.25 one is reported against the file.
package stdversion

import (
	"log/slog"
	"sync"
)

func tagged() {
	_ = slog.DiscardHandler
	var g sync.WaitGroup
	g.Go(func() {})
}
