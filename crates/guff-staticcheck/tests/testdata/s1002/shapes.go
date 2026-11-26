package main

// Every operand shape S1002 can meet, one per line, so the message can be
// pinned per shape. Upstream renders the operand with go/printer
// (`report.Render`), so the suggestion always quotes the real source; guff used
// to have a five-arm printer here and wrote the literal string `<expr>` for
// everything else — including `*corePP.KeepInputArtifact` on hashicorp/packer.

type S struct{ B bool }

func (s S) M() bool { return s.B }

func g() bool { return true }

var (
	arr [3]bool
	m   = map[string]bool{}
	ch  = make(chan bool, 1)
	i   any
)

func shapes(s S, b bool, ptr *bool, k string) {
	_ = b == true
	_ = b == false
	_ = !b == true
	_ = s.B == true
	_ = s.M() == true
	_ = g() == false
	_ = *ptr == false
	_ = arr[0] == true
	_ = m[k] == false
	_ = (<-ch) == true
	_ = (b && s.B) == false
	_ = i.(bool) == true
	_ = (b) == false
	_ = ((!b)) == true
}
