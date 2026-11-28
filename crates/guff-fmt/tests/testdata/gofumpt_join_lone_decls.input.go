package p

import "fmt"
import "os"

import (
	"strings"
)

// #include <stdio.h>
import "C"
import "errors"

var a = 1
var b = 2

var c = 3

var (
	d = 4
)

var e = 5
// doc for f
var f = 6

var g = 7

//go:embed x
var h string
var i = 8

func F() {
	fmt.Println(a, b, c, d, e, f, g, h, i, os.Args, strings.ToUpper(""), errors.New(""))
}

const k = 1
const l = 2
type T int
const m = 3
var n = 4
var o = 5 // trailing
var q = 6
var x = 9
//go:generate echo
var z = 10
