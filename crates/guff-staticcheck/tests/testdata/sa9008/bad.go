package main
import "fmt"
func f(x interface{}) {
  if v, ok := x.(int); ok { _ = v } else { fmt.Printf("%v", v) }
}
