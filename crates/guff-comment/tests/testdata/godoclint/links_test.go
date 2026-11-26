// Package links's test file also has a doc, and `no-unused-link` is pinned
// `include-tests: true` by golangci-lint — unlike `pkg-doc` and
// `start-with-name`, which are pinned false and skip this file entirely. A
// package doc that shares those rules' test guard loses this finding.
//
// [testpkgunused]: https://example.com/testpkgunused
package links

// TestHelper is a symbol in a test file.
//
// [testsymunused]: https://example.com/testsymunused
func TestHelper() {}
