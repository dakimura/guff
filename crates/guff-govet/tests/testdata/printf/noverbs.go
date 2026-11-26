package printf

import (
	"fmt"
	"log"
)

// A format string with no `%` in it at all is its own branch, before any
// parsing: upstream reports the leftover arguments at the **first argument
// after the format**, with its own wording, and returns. guff fell through to
// the arity check, which says `call needs 0 args but has N args` at the
// callee — a different message at a different position, so the two tools
// disagreed on a shape they both meant to report.
func noVerbs(x int, err error) {
	// Silent: nothing beside the format.
	fmt.Printf("no verbs here")
	// Reported, at the first argument.
	fmt.Printf("no verbs here", x)
	fmt.Printf("no verbs here", x, err)
	_ = fmt.Errorf("no verbs here", err)
	_ = fmt.Sprintf("no verbs here", x)
	// An empty format is also directive-free.
	fmt.Printf("", x)
	// `%%` is a percent, so this goes down the parsing path instead.
	fmt.Printf("100%% done", x)
	// And so does an ordinary arity mismatch.
	fmt.Printf("%d", x, x)
}

// The name in the message is `types.Func.FullName()`, so a method carries its
// receiver: `(*log.Logger).Printf`, never `log.Printf`.
func methodNames(l *log.Logger, x int) {
	l.Printf("no verbs here", x)
}
