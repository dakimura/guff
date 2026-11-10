// Not upstream: the shapes of unnecessary-format's callee and format literal.
package shapes

import (
	"fmt"
	"log"
)

type wrap struct{ fmt struct{ Errorf func(string) } }

func shapes(w wrap) {
	_ = fmt.Errorf("\x25d", 1) // the value contains a %
	_ = fmt.Errorf("%d", 1)    // the same as a unicode escape
	_ = fmt.Errorf(`raw`)      // a raw string without a %
	_ = (fmt.Errorf)("paren")  // a parenthesized callee prints as no key
	_ = (fmt).Errorf("paren")  // the same
	w.fmt.Errorf("chain")      // a longer selector prints as no key
	_ = fmt.Errorf(("inner"))  // a parenthesized format is no *ast.BasicLit
	logger := log.Default()
	logger.Printf("by name") // matched by the variable's name
	l := log.Default()
	l.Printf("other name") // not a key
	fmt.Printf('x')        // a rune is no string literal
	_ = fmt.Sprintf("")    // an empty format
}
