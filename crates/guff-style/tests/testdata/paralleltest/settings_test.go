package paralleltest

import "testing"

func TestMissingButIgnored(t *testing.T) {
	t.Error("ignored by settings")
}

func TestCleanupDefer(t *testing.T) {
	t.Parallel()
	defer func() {}()
	t.Error("cleanup")
}
