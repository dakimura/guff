package indexes

import "fmt"

// An explicit index binds to whichever position absorbs it — a width `*`, a
// precision `*`, or the verb — and each `*` operand must be an int. rclone's
// `fmt.Sprintf("%[2]*[1]s", str, rawWidth)` is the first line here: the width
// comes from argument 2 and the string from argument 1.
func indexes(str string, w int) {
	fmt.Printf("%[2]*[1]s", str, w)
	fmt.Printf("%[2]*[1]d", str, w)
	fmt.Printf("%[1]*[2]s", str, w)
	fmt.Printf("%*s", w, str)
	fmt.Printf("%*s", str, str)
	fmt.Printf("%.*f", str, 1.0)
	fmt.Printf("%[2]*.[1]*[2]d", w, w)
	fmt.Printf("%[2]*d", w, str)
	fmt.Printf("%.[2]*[1]d", w, w)
	fmt.Printf("%[1]*[1]d", w)
	fmt.Printf("%.*[2]d", w, w)
	fmt.Printf("%-36[1]s|", str)
	fmt.Printf("%[3]d", 1, 2)
}

// Upstream parses the whole format string before checking any argument, so a
// malformed directive is the only thing reported, and it is quoted back
// verbatim.
func malformed(str string) {
	fmt.Printf("%d %[x]d", str)
	fmt.Printf("%[0]d", 1)
	fmt.Printf("%[]d", 1)
	fmt.Printf("%[-1]d", 1)
	fmt.Printf("%[999999999999]d", 1)
	fmt.Printf("a %[3d b %s", str)
	fmt.Printf("%[1]", 1)
}

// Only the first mistake in a format string is reported.
func onlyFirst(str string, s2 string) {
	fmt.Printf("%d %d", str, s2)
	fmt.Printf("%d %s", str)
	fmt.Printf("%z %d", str, str)
}
