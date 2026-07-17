package testableexamples_ok

import "fmt"

func Example_good() {
	fmt.Println("hello")
	// Output: hello
}

func Example_empty() {
	// Output:
}

func Example_unordered() {
	fmt.Println("x")
	// Unordered output: x
}
