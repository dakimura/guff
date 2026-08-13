package main

type E int

// Upstream groups by `astutil.GroupSpecs`: two specs share a group only when
// they are on consecutive lines. Both constants below are adjacent, so they are
// one group, and only the first carries a type — reported.
//
// Verified against golangci-lint 2.12.2 alongside the three shapes in `ok.go`,
// which are the ones that leave the group with a single member.
const (
	A E = 1
	B   = 2
)

func main() {
	_ = A
	_ = B
}
