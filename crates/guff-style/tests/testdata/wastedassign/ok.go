package wastedassign_ok

import "fmt"

func usedAfterAssign() {
	a := 0
	fmt.Print(a)
	a = 1
	fmt.Print(a)
}

func usedBetweenReassign() {
	b := 0
	fmt.Print(b)
	b = 1
	fmt.Print(b)
	b = 2
	fmt.Print(b)
}
