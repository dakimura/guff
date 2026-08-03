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
