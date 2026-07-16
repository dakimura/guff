package gocritic

import (
	"flag"
	"log"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
)

func elseIf(cond1, cond2 bool) {
	if cond1 {
		println("a")
	} else {
		if cond2 {
			println("b")
		}
	}
}

func singleCase(x int) {
	switch x {
	case 1:
		println(1)
	}
}

func defaultMiddle(x int) {
	switch x {
	case 1:
		println(1)
	default:
		println("d")
	case 2:
		println(2)
	}
}

func switchTrue() {
	switch true {
	case true:
		println("t")
	}
}

func sloppy(s []int) {
	_ = len(s) >= 0
	_ = len(s) < 0
	_ = len(s) <= 0
}

func unslice(s []int) {
	_ = s[:]
}

func newDeref() {
	_ = *new(bool)
}

func appendAssign(xs, ys []int) {
	xs = append(ys, 1)
}

func dupCase(x int, ys []int) {
	switch x {
	case ys[0], ys[1], ys[0]:
		println(x)
	}
}

func captLocal(IN int) (OUT int) {
	return IN
}

func exitDefer(name string) {
	defer os.Remove(name)
	log.Fatal("boom")
}

func ifElseChain(a, b, c bool) {
	if a {
		println(1)
	} else if b {
		println(2)
	} else {
		println(3)
	}
}

func valSwap(x, y int) {
	tmp := y
	y = x
	x = tmp
}

func flagDeref() {
	_ = *flag.Bool("b", false, "docs")
}

func badCall(s string) {
	_ = append([]byte(nil))
	_ = filepath.Join("only")
	_ = strings.Replace(s, "a", "b", 0)
}

func assignOp(x int) {
	x = x + 1
	x = x * 2
}

func underef(p *struct{ N int }) {
	_ = (*p).N
}

func dupArg(a string) {
	_ = strings.Contains(a, a)
}

func dupBranch(cond bool) {
	if cond {
		println("same")
	} else {
		println("same")
	}
}

func dupSub(x int) {
	_ = x < x
}

func flagNameBad() {
	_ = flag.Bool(" foo ", false, "docs")
}

func mapKeyBad() {
	_ = map[string]int{
		"foo":  1,
		"bar ": 2,
	}
}

func offBy1(xs []int) {
	_ = xs[len(xs)]
}

func typeSwitchVar(v interface{}) int {
	switch v.(type) {
	case int:
		return v.(int)
	default:
		return 0
	}
}

func badCondFor(n int) {
	for i := 0; i > n; i++ {
		_ = i
	}
}

func badCondExpr(x, a, b int) {
	_ = x == a && x == b
}

func unlambda(fn func(int) int) {
	_ = func(x int) int { return fn(x) }
}

func regexpMust() {
	_, _ = regexp.Compile("abc")
}

func wrapperFunc(s string, wg *sync.WaitGroup) {
	_ = strings.SplitN(s, ",", -1)
	wg.Add(-1)
}

func argOrder(s string) {
	_ = strings.HasPrefix("#", s)
}
