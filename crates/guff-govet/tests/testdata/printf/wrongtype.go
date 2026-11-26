package wrongtype

import "fmt"

func f() {
	fmt.Printf("%d", "str")
	fmt.Printf("%s", 42)
	fmt.Printf("%t", 5)
}
