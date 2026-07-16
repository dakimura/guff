package gocritic

import (
	"flag"
	"log"
	"os"
	"path/filepath"
	"strings"
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
