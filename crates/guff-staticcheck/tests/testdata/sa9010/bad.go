package main
func closer() func() { return func() {} }
func f() { defer closer() }
