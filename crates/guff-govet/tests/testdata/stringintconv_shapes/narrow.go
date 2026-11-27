//go:build go1.21

package extra

import (
	f "fmt"

	"example.com/govet/stringintconv/shapes/dep"
)

var _ = f.Sprintf

type small uint16

func (small) Format(f.State, rune) {}

type G[T any] int

func (G[T]) String() string { return "" }

func _(s small, g G[int], u8 uint8, u16 uint16, n dep.Num, c int32) {
	println(string(s))         // value method: rune(s) only
	println(string(g))         // generic named, value method, int: no fix
	println(string(u8))        // byte: silent
	println(string(u16))       // rune(u16) + %d
	println(dep.Str(n))        // selector target, named source from another package
	println((string)(u16))     // parenthesized: upstream does not look
	println(string(c))         // rune: silent
	println(string(65))        // untyped const fits
	println(string(-1))        // negative untyped const fits int32
	println(string(1 << 31))   // untyped const past int32: no fix
	println(string(int64(70))) // typed const that fits
}
