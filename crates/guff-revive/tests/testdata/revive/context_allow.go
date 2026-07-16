package revivetest

import (
	"context"
	"testing"
)

// Allowed when allowTypesBefore includes *testing.T.
func withTestingT(t *testing.T, ctx context.Context) {
	_ = t
	_ = ctx
}

// Allowed when allowTypesBefore includes testing.TB.
func withTestingTB(tb testing.TB, ctx context.Context) {
	_ = tb
	_ = ctx
}

// Still flagged: plain int is not in allowTypesBefore.
func stillBad(x int, ctx context.Context) {
	_ = x
	_ = ctx
}
