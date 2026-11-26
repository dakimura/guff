package fixtures

import "fmt"

// Not upstream: ignoreFuncs applies to the call's own subtree only.
func extra() {
	fmt.Println(func() int { return 77 }()) // the FuncLit is walked with the visitor, unignored
	fmt.Println(func() { _ = 78 })          // the same for a FuncLit argument
	fmt.Println(3 + 81)                     // ignored: a binary expression in an ignored call
	fmt.Println(notIgnoredFunc(79))         // a nested call applies its own setting
	fmt.Println([]int{80})                  // ignored
	x := 2                                  // allowed
	y := 2.0                                // not in the allow list (only 0.0 and 1.0)
	_, _ = x, y
	getLogger().Info(nil, fmt.Sprint(82)) // `.Info` is ignored; fmt.Sprint is not
}
