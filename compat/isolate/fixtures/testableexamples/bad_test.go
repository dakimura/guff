package p

import "fmt"

// testableexamples says the same sentence about every example that has no
// output comment, but it walks four *kinds* of example: package, function,
// type, and method. Each is a separate lookup upstream.

func Example() {
	fmt.Println("package example")
}

func ExampleBad() {
	fmt.Println("function example")
}

type T struct{}

func ExampleT() {
	fmt.Println("type example")
}

func ExampleT_Method() {
	fmt.Println("method example")
}
