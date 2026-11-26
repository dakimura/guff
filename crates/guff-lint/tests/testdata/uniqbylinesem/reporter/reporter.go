// Package reporter puts two linters on one line, several ways.
package reporter

import (
	"fmt"
	"os"
)

func Loop() []*int {
	var out []*int
	for _, v := range []int{1, 2, 3} {
		out = append(out, &v)          // gosec G601 + revive range-val-address (reporter, reporter)
		go func() { fmt.Println(v) }() // govet loopclosure + revive range-val-in-closure (diag, reporter)
	}
	return out
}

func Errs() {
	os.Remove("x") // errcheck + gosec G104 (reporter, reporter)
	fmt.Printf("%d", "s") // govet printf + staticcheck? (diag, diag)
}
