// Package settings is the fixture for revive's `confidence` and `severity`
// keys rather than for any one rule.
//
// revive attaches a confidence to every failure and golangci-lint drops the
// ones below `revive.confidence` (default 0.8), so a fixture that can measure
// the key needs findings at more than one confidence. These three are a
// ladder, one rule per rung:
//
//	increment-decrement  0.8
//	error-naming         0.9
//	errorf               1
//
// Raising the threshold past each rung takes exactly one finding away, which is
// what the four `revive-confidence-*` goldens are.
package settings

import (
	"errors"
	"fmt"
)

// BadError is misnamed on purpose: error-naming wants errFoo / ErrFoo.
var BadError = errors.New("bad")

// Count increments the long way (increment-decrement).
func Count(i int) int {
	i += 1
	return i
}

// Wrap builds an error the long way (errorf).
func Wrap(name string) error {
	return errors.New(fmt.Sprintf("bad %s", name))
}

// The declaration below is undocumented on purpose: `exported` is one of
// revive's *default* rules and is not on the three-rule list the baseline
// names, so it is the finding that separates `enable-default-rules` from a
// plain `rules` list. This comment is detached from it by the blank line, so
// it is not a doc comment.

func Exported() {}
