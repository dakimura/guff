// Test files are looked at: golangci-lint pins gosmopolitan's `lookattests`
// to true, overriding the linter's own `LookAtTests: false` default.
//
// No `testing` import — the stub universe here does not carry one, and the
// only thing that ever mattered is the file's name.
package gosmopolitan

var hanInATestFile = "你好"
