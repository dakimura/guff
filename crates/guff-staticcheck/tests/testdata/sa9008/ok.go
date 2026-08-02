package main
import "fmt"
func f(x interface{}) {
  if v, ok := x.(int); ok { _ = v } else { fmt.Printf("%v", x) }
}
func g(x interface{}) {
  // !ok then-branch is the failure path; else uses the asserted value — not SA9008.
  if v, ok := x.(int); !ok {
    fmt.Printf("fail")
  } else {
    fmt.Printf("%v", v)
  }
}
