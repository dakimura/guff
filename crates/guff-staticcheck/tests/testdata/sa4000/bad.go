package main
func main() {
    x := 1
    _ = x == x
    _ = 1 == 1
    // The '|' arm of the operator list, which the type-parameter unions in
    // ok.go reach through a node kind the renderer used to flatten.
    _ = x | x
}
