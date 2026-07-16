package gocritic

import (
	"fmt"
	formatting "fmt"
	"path/filepath"
	// "os"
)

func emptyStringExtra(s string) {
	_ = len(s) == 0
	_ = len(s) != 0
}

func emptyFallthroughExtra(i int) {
	switch i {
	case 0:
		fallthrough
	case 1:
		_ = i
	}
}

func emptyDeclExtra() {
	var ()
	const ()
	type ()
}

func octalLiteralExtra() {
	_ = 0755
}

func nilValReturnExtra(err error) error {
	if err == nil {
		return err
	}
	return nil
}

func yodaStyleExtra(p *int) {
	if nil == p {
	}
	if 10 == *p {
	}
}

func deferUnlambdaExtra() {
	defer func() { fmt.Println("hello") }()
	formatting.Println("alias")
}

func initClauseExtra() {
	if sideEffectExtra(); true {
	}
}

func sideEffectExtra() {}

func builtinShadowExtra(len int) {
	_ = len
}

func paramCombineExtra(a int, b int) {}

func filepathJoinExtra(name string) {
	_ = filepath.Join("dir/", name)
}

func rangeAppendExtra(ns []int) {
	var rs []int
	for _, n := range ns {
		_ = n
		rs = append(rs, ns...)
	}
	_ = rs
}

func weakCondExtra(xs []int) {
	_ = xs != nil && xs[0] != 0
}

func complex64() {}
