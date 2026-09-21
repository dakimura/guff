//go:build go1.25

// The three modernize checks that read a statement's *neighbour* rather than
// the statement itself, written once in each list a statement can sit in: a
// block body, a `switch` case body, and a `select` comm clause body.
//
// guff dispatched all three from `BlockStmt` alone, so the `case` and `select`
// spellings were invisible. Every shape here was measured against
// golangci-lint 2.12.2 before it was written down.
package stmtlist

import "sync"

// -- minmax pattern 2: `x = a` immediately above `if a < b { x = b }`.

func minmaxBlock(a, b int) int {
	x := a
	if a < b {
		x = b
	}
	return x
}

func minmaxCase(a, b, k int) int {
	var x int
	switch k {
	case 1:
		x = a
		if a < b {
			x = b
		}
	}
	return x
}

func minmaxComm(a, b int, ch chan int) int {
	var x int
	select {
	case <-ch:
		x = a
		if a < b {
			x = b
		}
	}
	return x
}

// Silent. The assignment above the `if` is the clause's `Comm`, not a
// statement of its body, and a comm clause cannot be rewritten into a
// `min`/`max` call. Upstream rejects `edge.CommClause_Comm` by name
// (`minmax.go:160`); here the `Comm` is simply not in the slice we walk.
func minmaxCommIsNotAStatement(b int, ch chan int) int {
	var x int
	select {
	case x = <-ch:
		if x < b {
			x = b
		}
	}
	return x
}

// -- slicescontains: the loop's neighbour decides which rewrite is offered.

// Next sibling is `return false`, so the whole loop collapses to a `return`.
func containsCaseReturn(s []int, needle, k int) bool {
	switch k {
	case 1:
		for _, v := range s {
			if v == needle {
				return true
			}
		}
		return false
	}
	return false
}

// Previous sibling is `found = false`, so the loop collapses into it.
func containsCaseAssign(s []int, needle, k int) bool {
	var found bool
	switch k {
	case 1:
		found = false
		for _, v := range s {
			if v == needle {
				found = true
				break
			}
		}
	}
	return found
}

func containsCommAssign(s []int, needle int, ch chan int) bool {
	var found bool
	select {
	case <-ch:
		found = false
		for _, v := range s {
			if v == needle {
				found = true
				break
			}
		}
	}
	return found
}

// The shape beats writes: the loop is the body of a type-switch case, and the
// slice it ranges over is the case's binding.
func containsTypeSwitch(list any, msg string) bool {
	switch v := list.(type) {
	case []string:
		for _, tag := range v {
			if msg == tag {
				return true
			}
		}
	}
	return false
}

// -- waitgroupgo: `wg.Add(1)` immediately above the `go` statement.

func waitgroupBlock(wg *sync.WaitGroup) {
	wg.Add(1)
	go func() {
		defer wg.Done()
		_ = 1
	}()
}

func waitgroupCase(wg *sync.WaitGroup, k int) {
	switch k {
	case 1:
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = 1
		}()
	}
}

func waitgroupComm(wg *sync.WaitGroup, ch chan int) {
	select {
	case <-ch:
		wg.Add(1)
		go func() {
			defer wg.Done()
			_ = 1
		}()
	}
}
