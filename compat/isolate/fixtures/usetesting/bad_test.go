package p

import (
	"os"
	"testing"
)

func TestBad(t *testing.T) {
	_, _ = os.MkdirTemp("", "x")
}

// Upstream inspects the whole body of a function whose first parameter is a
// `*testing.T`, closures included, and names *that* function in the message —
// so a call buried in a parameterless closure is still reported, against the
// enclosing `t`. gitea `tests/integration/attachment_test.go`.
func testHelperWithClosure(t *testing.T) {
	count := func() string {
		return os.TempDir()
	}
	_ = count()
}

// A subtest literal has a parent function, so upstream skips it as a function
// of its own: the finding belongs to `TestSubtest` and names its `t`, not the
// literal's.
func TestSubtest(t *testing.T) {
	t.Run("sub", func(t *testing.T) {
		_ = os.TempDir()
	})
}

// A literal with no enclosing function is checked on its own, and is the one
// case that reports as "anonymous function".
var _ = func(t *testing.T) {
	_ = os.TempDir()
}
