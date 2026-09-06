package p

import "testing"

// golangci-lint pins gosmopolitan's `lookattests` to **true**
// (`pkg/golinters/gosmopolitan/gosmopolitan.go`, commented "Should be managed
// with `linters.exclusions.rules`"), overriding the linter's own
// `LookAtTests: false` default. So a test file is looked at like any other,
// and skipping `*_test.go` cost guff cert-manager's two
// `//nolint: gosmopolitan` directives — which it then reported as unused.
func TestHanInATestFile(t *testing.T) {
	if "你好" == "" {
		t.Fatal("unreachable")
	}
}
