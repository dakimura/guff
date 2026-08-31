package main

type R struct{ Complete bool }

func beforeCheck(p *R) {
	_ = p.Complete // want
	if p != nil {
		_ = p
	}
}

type TB interface {
	Fatal(args ...interface{})
	Fatalf(format string, args ...interface{})
}

// Matches golangci SA5011 on sequential testing.TB Fatal (vault :414):
// interface Fatal is not noreturn, so the use is still reported.
func sequentialFatal(t TB, statusResp *R) {
	if statusResp == nil {
		t.Fatal("nil")
	}
	_ = statusResp.Complete // want
}

type cl struct{ servers []int }

func newCl() *cl            { return nil }
func useAny(...interface{}) {}

// The control for ok.go's okDerefInThenArm: put the *deref* below the join too
// and nothing renames it — the check compares the same value in the same block,
// so upstream reports. Narrowing the branch rule must not reach this.
func derefBelowJoinThenCheck(cond bool) {
	c := newCl()
	if cond {
		useAny("y")
	} else {
		useAny("x")
	}
	useAny(c.servers) // want
	if c != nil {
		useAny(c)
	}
}
