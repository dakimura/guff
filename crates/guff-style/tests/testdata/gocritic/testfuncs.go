package gocritic

import "testing"

// rangeValCopy and rangeExprCopy are the only two checkers with a
// `skipTestFuncs` param, and it defaults to **true** — so this is default
// behaviour, not an unwired setting. Their `EnterFunc` returns false for a
// unit test function, which prunes the whole subtree.
//
// `isUnitTestFunc` is name + signature, never the file name: "Test" prefix,
// one `*testing.T` parameter, no results. Each function below sits on one side
// of that definition, so the golden records where upstream draws the line.
// k9s put three of these in `internal/render/*_test.go`.

type bigRow struct {
	pad [200]byte
}

// Skipped: a unit test function.
func TestRangeValCopySkipped(t *testing.T) {
	rows := make([]bigRow, 3)
	for _, r := range rows {
		_ = r
	}
	// Nested closures are pruned with the parent.
	t.Run("sub", func(t *testing.T) {
		for _, r := range rows {
			_ = r
		}
	})
}

// Not skipped: a benchmark takes *testing.B, so it is not a unit test func.
func BenchmarkRangeValCopyReported(b *testing.B) {
	rows := make([]bigRow, 3)
	for _, r := range rows {
		_ = r
	}
}

// Not skipped: the "Test" prefix is not enough — this one has a result.
func TestWithResultReported(t *testing.T) error {
	rows := make([]bigRow, 3)
	for _, r := range rows {
		_ = r
	}
	return nil
}

// Not skipped: an ordinary helper, even though it lives beside the tests.
func rangeValCopyHelperReported() {
	rows := make([]bigRow, 3)
	for _, r := range rows {
		_ = r
	}
}

// rangeExprCopy takes the same param: ranging over a big *array* (not a slice)
// copies the expression itself.
func TestRangeExprCopySkipped(t *testing.T) {
	var rows [16]bigRow
	// Both key and value: rangeExprCopy returns early without them.
	for i, v := range rows {
		_, _ = i, v
	}
}

func rangeExprCopyHelperReported() {
	var rows [16]bigRow
	// Both key and value: rangeExprCopy returns early without them.
	for i, v := range rows {
		_, _ = i, v
	}
}
