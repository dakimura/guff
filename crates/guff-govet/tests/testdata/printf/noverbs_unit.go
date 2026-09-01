package printf

import (
	"fmt"
	"log"
	"testing"
)

// The unit-test copy of `noverbs.go`, with the stub packages this harness
// type-checks against. The golden case runs the real fixture against the real
// standard library; this one is here so the shapes are still measured where
// golangci-lint is not installed.
func noVerbsUnit(x int, err error, l *log.Logger, t *testing.T) {
	fmt.Printf("no verbs here")
	fmt.Printf("no verbs here", x)
	_ = fmt.Errorf("no verbs here", err)
	_ = fmt.Sprintf("no verbs here", x)
	fmt.Printf("", x)
	fmt.Printf("%d", x, x)
	l.Printf("no verbs here", x)
	t.Errorf("no verbs here", x)
	t.Logf("no verbs here", x)
	t.Fatalf("no verbs here", x)
}
