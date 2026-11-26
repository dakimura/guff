package istest

import (
	"os"
	"testing"
)

// A test *file* in a non-test package — the ordinary internal test file, which
// asking the package name calls "not a test". unsecure-url-scheme skips it and
// deep-exit exempts TestMain here.
const testEndpoint = "http://example.com/test"

func TestMain(m *testing.M) {
	os.Exit(m.Run())
}

func TestEndpoint(t *testing.T) {
	if testEndpoint == "" {
		t.Fatal("empty")
	}
}
