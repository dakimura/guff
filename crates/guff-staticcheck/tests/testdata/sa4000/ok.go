package main
func main() {
    x, y := 1, 2
    _ = x == y
    // Distinct composite lits must not collapse to identical `<expr>` renders.
    type id struct{ Name, Group, Kind string }
    a, b := id{Name: "dash-1", Group: "g", Kind: "K"}, id{Name: "dash-2", Group: "g", Kind: "K"}
    _ = a.Name == b.Name && a.Group == b.Group
}
