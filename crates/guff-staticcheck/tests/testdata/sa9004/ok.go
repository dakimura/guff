package main

type E int

// Every constant carries its own type: nothing to report even though the two
// specs are adjacent and therefore in one group.
const (
	A E = 1
	B E = 2
)

// A doc comment between the specs splits the group, because `ValueSpec.Pos()`
// is the first name and starts *below* the comment — so each group has one
// member and the check never runs. This is prometheus `tsdb/head.go:239-242`
// reduced (COMPAT-HARDENING §4, 2026-08-13); guff used to report it.
const (
	C E = 1
	// doc comment between the two constants
	D = 2
)

// Same rule from the other side: a blank line splits the group.
const (
	P E = 1

	Q = 2
)

// And specs on the *same* line are not adjacent either — `End().Line + 1` can
// never equal `Pos().Line` when both are on one line, so upstream splits here
// too. `bad.go` used to be written in this form, which is why it was asserting
// a finding upstream does not make.
const ( X E = 1; Y = 2 )

func main() {
	_ = A
	_ = B
	_ = C
	_ = D
	_ = P
	_ = Q
	_ = X
	_ = Y
}
