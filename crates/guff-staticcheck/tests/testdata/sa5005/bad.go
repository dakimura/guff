package main
import "runtime"
func f() { x := new(int); runtime.SetFinalizer(x, func() { _ = x }) }
