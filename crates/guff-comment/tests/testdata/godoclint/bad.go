// This package is an example.
package example

// Bar does something.
const Bar = 1

// This is Foo.
func Foo() {}

// Quux is deprecated.
//
// DEPRECATED: use Bar
func Quux() {}

const (
	Alpha = 1
	// This is Beta, and the doc comment is indented by a tab.
	//
	// Upstream reports where the comment starts, so this one is at column 2.
	// Recovering only the line and reporting the line's start says column 1,
	// which is right for every declaration at the left margin and wrong here.
	Beta = 2
)

type Wrapper struct{}

var (
	// This is Gamma, indented inside a var group.
	Gamma = Wrapper{}
)
