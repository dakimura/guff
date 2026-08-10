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

	// A non-ASCII verb: upstream decodes a rune here, not a byte, so this is
	// one unknown verb rather than the first byte of one.
	fmt.Printf("%é", 1)
	fmt.Printf("%☃", 1)
	// An ill-formed byte in the format decodes to U+FFFD for the verb, and to
	// nothing at all for the column: upstream's own offset walk advances by
	// utf8.RuneLen and so counts "\xff" as two bytes.
	fmt.Printf("%\xff", 1)
	fmt.Printf("\xff%d %d", 1)
}
