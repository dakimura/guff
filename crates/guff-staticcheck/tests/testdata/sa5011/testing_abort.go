// Package pkg exercises SA5011 against calls that abort.
package pkg

import "testing"

type Acc struct{ sl *Sub }
type Sub struct{ n int }

func lookup(name string) (*Acc, error) { return nil, nil }

// `ctrlflow` proves `(*testing.T).Fatal` never returns — it reaches
// `runtime.Goexit` — so upstream's IR drops the edge out of the abort block.
// The code below the `if` is then entered only from the non-nil side, and a
// block with a single predecessor gets a sigma for every value live below it,
// which SA5011's value-identity test can never match.
//
// The short-circuit spelling is the one dominance alone does not cover, since
// the abort is reachable from either condition. nats-server writes it in three
// test files (`server/accounts_test.go:402`).
func orFatal(t *testing.T) {
	fooAcc, _ := lookup("foo")
	barAcc, _ := lookup("bar")
	if fooAcc == nil || barAcc == nil {
		t.Fatalf("missing")
	}
	if fooAcc.sl == nil || barAcc.sl == nil {
		t.Fatal("missing sublists")
	}
}

// The receiver decides whether the call aborts, not the enclosing function: a
// `func(k, v any) bool` callback takes interfaces, but the `Fatal` inside it is
// still a static call on a captured concrete `*testing.T`. Reading the
// enclosing signature instead reported nats-server `server/opts_test.go:2731`.
func fatalInsideCallback(t *testing.T, each func(func(k, v any) bool)) {
	each(func(k, v any) bool {
		acc, _ := v.(*Acc)
		if acc == nil {
			t.Fatalf("nil account")
		}
		return acc.sl == nil
	})
}
