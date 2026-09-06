//go:build go1.18

// This file's language version is 1.18, so range-over-int — a 1.22 feature —
// does not exist here and modernize must not offer it, however new the module
// is. `code::stdlib_version` answers `max(module, file)`, which is right for
// asking how new the *standard library* may be and wrong for every gate in
// this suite; flipt's `internal/storage/fs/config_fuzz_test.go` is this file.
package modernize

// modernize is silent here; `intrange` is not, and that pairing is the point.
// The two linters disagree about the same loop in the same file, so this
// fixture pins both — and `intrange` firing alone is what first applied its
// `--fix`, which until then had always been overwritten by modernize's and was
// writing `for i i := range n`.
func rangeIntBelowGo122(n int) {
	for i := 0; i < n; i++ {
		_ = i
	}
}
