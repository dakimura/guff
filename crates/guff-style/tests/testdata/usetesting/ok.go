package usetesting

import (
	"os"
	"testing"
)

func TestOk(t *testing.T) {
	dir := t.TempDir()
	_, _ = os.CreateTemp(dir, "prefix")
}

func NotATest() {
	_, _ = os.MkdirTemp("", "prefix")
}
