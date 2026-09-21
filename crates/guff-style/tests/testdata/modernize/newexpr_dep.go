//go:build go1.26

// The `call of F(x)` arm when `F` is declared in a package guff did not
// analyse.
//
// Upstream exports a `newLike` fact while analysing the declaring package, and
// golangci-lint runs the analyzer over dependencies, so a wrapper in another
// module arrives as a fact. guff analyses the run set and imports the rest
// from export data, so the fact never comes — azcore's `to.Ptr` and govmomi's
// `NewBool` were 42 of beats' diffs on that alone.
//
// The signature is in export data; the only thing missing is whether the body
// is `return &x`, and the dependency's sources are already on disk.
package modernize

import "example.com/dep/to"

func newexprDepPtr() *string { return to.Ptr("x") }

func newexprDepBool() *bool { return to.Ptr(true) }

// Silent: the body takes the address of a copy.
func newexprDepNotNewLike() *string { return to.NotNewLike("x") }

// Silent: two parameters.
func newexprDepTwoParams() *string { return to.TwoParams("x", "y") }

// Silent: the address is not the parameter's.
func newexprDepShared() *int { return to.Shared(1) }
