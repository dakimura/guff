package session

import (
	"example.com/mutual/tool"
	"example.com/mutual/vault"
)

// Helper is declared in an in-package _test.go, so it exists only in
// session [session.test] — which is exactly what the seed compiles for a path
// that has an external test package.
type Helper struct {
	tool.T
}

// session's test imports vault; vault's test imports session. Legal Go, and the
// cycle guff's one-node-per-path seed has to decline an edge of.
var _ = func() string { return vault.New("x").GetPublicId() }
