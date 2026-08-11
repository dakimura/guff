// Package gover is the `run.go` fixture. Its module is `go 1.21` on purpose:
// the checks that read a Go version are the ones that only apply *below* some
// version, so a module at the current version can never show the setting
// working.
//
// `run.go` is not a filter on the source — it is a value the loader pushes into
// several linters (`Settings.Govet.Go`, `Settings.Revive.Go`,
// `Settings.Gocritic.Go`, gofumpt's `-lang`, `GOSECGOVERSION`). Raising it to
// 1.22 here removes findings from a file the toolchain still compiles as 1.21.
package gover

func mkerr() error { return nil }

// Run has one errcheck finding, which no Go version gates: it is what keeps
// both goldens non-empty so the diff is only the version-gated checks.
func Run() {
	mkerr()
}

// Loop is the pre-1.22 loop-variable capture. `govet/loopclosure` is dropped
// from the analyzer set outright when the configured Go version is at least
// 1.22 (`govet.go`: `name == loopclosure.Analyzer.Name &&
// IsGoGreaterThanOrEqual(cfg.Go, "1.22")`), and revive's `range-val-in-closure`
// and `range-val-address` ask their own `IsAtLeastGoVersion(Go122)`.
func Loop() []*int {
	var out []*int
	for _, v := range []int{1, 2, 3} {
		out = append(out, &v)
		// The `go` statement has to be the last one in the body: x/tools'
		// loopclosure only reports a capture it can prove outlives the
		// iteration, which it does by looking at the tail of the loop.
		go func() {
			_ = v
		}()
	}
	return out
}
