package escapes

import "fmt"

// printf reports at the `%v` substring inside the format string, so the
// position has to be mapped from an offset in the *decoded* string back
// through the literal's escape sequences.
func f() {
	fmt.Printf("\t%z", 1)
	fmt.Printf("é\n%z", 1)
	fmt.Printf("\x41\101%z", 1)
	fmt.Printf(`raw %z`, 1)
}
