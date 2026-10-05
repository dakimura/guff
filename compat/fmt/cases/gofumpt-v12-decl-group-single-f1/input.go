package p

import "non-grouped"

import (
	"grouped"
)

var single = "foo"

var (
	// verbose is verbose.
	verbose = "bar"
)

// This entire block has a comment.
var (
	groupComment = "bar"
)

var (
	multiple1 string
	multiple2 string
)

const (
	first = iota
)

var (
	multiline = []string{
		"foo",
		"bar",
	}
)

var (
	foo = "foo"
	// bar = "bar"
	// baz = "baz"
)
