//go:build go1.24

package modernize

import (
	"context"
	"testing"
)

func TestWithCancel(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	_ = ctx
}
