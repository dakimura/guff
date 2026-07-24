package wastedassign

import "fmt"

func wastedNeverUsed() {
	a := 0
	fmt.Print(a)
	a = 1
}

func wastedReassigned() {
	b := 0
	fmt.Print(b)
	b = 1
	b = 2
	fmt.Print(b)
}
