package main

// Local Printf stand-in so the smoke test does not depend on stdlib export data.
func Printf(format string, args ...any) {}

func main() {
	Printf("%z", 42)
}
