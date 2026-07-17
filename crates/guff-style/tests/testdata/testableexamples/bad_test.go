package testableexamples

import "fmt"

func Example_good() {
	fmt.Println("hello")
	// Output: hello
}

func Example_goodEmptyOutput() {
	fmt.Println("")
	// Output:
}

func Example_bad() {
	fmt.Println("hello")
}

func Example_unorderedOk() {
	fmt.Println("a")
	// Unordered output: a
}
