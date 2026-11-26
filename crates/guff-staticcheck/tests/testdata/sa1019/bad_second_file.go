package main

// A *second* file importing the same block-comment-deprecated package.
//
// One file alone cannot see the defect: its import spec is visited before its
// call, so the report is already made when the call forces a full object scan
// of the dependency. That scan re-reads every file behind a byte probe, and
// while the probe knew only `// Deprecated:` and `* Deprecated:` it rejected
// the block-comment doc — rebuilding the cached facts *without* the package
// deprecation. Every later import spec then found a cache that said "not
// deprecated".
//
// `typecheck_rule` loads one file, so this shape is gated by
// `compat/golden/cases/staticcheck-sa` (both files are materialized into the
// same directory on purpose, which is the one place that case wants a package
// of two files).

import "example.com/blockdoc"

func secondFile() {
	blockdoc.H()
}
