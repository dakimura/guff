package argcount

import "fmt"

func f() {
	fmt.Printf("%d %d", 1)
	fmt.Printf("%d", 1, 2)
}
