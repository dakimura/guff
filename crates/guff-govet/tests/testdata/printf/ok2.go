package ok2

import "fmt"

type S struct{}

func (S) String() string { return "s" }

type MyInt int

func f() {
	fmt.Printf("%s", S{})               // Stringer
	fmt.Printf("%s", []byte("hi"))      // []byte for %s
	fmt.Printf("%d", []int{1, 2})       // element-wise
	fmt.Printf("%d", MyInt(3))          // named int
	fmt.Printf("%[2]d %[1]s", "a", 2)   // indexed
	fmt.Printf("%*d", 4, 2)             // star width
	fmt.Printf("%v", S{})               // %v anything
	_ = fmt.Sprintf("%s", "x")
}
