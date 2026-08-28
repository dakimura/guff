// Package example is a well-documented package.
package example

// Foo does something useful.
func Foo() {}

// A Bar is an integer constant.
const Bar = 1

// Quux is deprecated.
//
// Deprecated: use Foo instead.
func Quux() {}

// Zed does a thing.
//
//	deprecated: this line is indented, so go/doc/comment parses it as a code
//	block rather than a paragraph, and a deprecation marker only counts at the
//	start of a paragraph.
func Zed() {}

// Yon does a thing.
//
// # deprecated: a heading is not a paragraph either
//
// More prose.
func Yon() {}

// Wye is a symbol but here are the reasons why it is
// deprecated: this begins a line, not a paragraph, so upstream leaves it
// alone rather than risk flagging ordinary prose.
func Wye() {}
