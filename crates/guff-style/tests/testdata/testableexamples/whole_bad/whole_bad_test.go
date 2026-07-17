package whole_bad

import "fmt"

func Example_wholeFileBad() {
	doBad("hello")
}

func doBad(s string) {
	fmt.Println(s)
}
