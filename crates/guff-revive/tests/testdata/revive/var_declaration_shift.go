// Package vardeclshift is var-declaration over shifts. A shift has the type of
// its left operand, so `1 << shift` is an untyped constant with default type
// int, whatever `shift` is: the declared type is redundant only when it is int.
// Each line's comment is what golangci-lint 2.12.2 does.
package vardeclshift

var shift uint = 3

func shifts() {
	var a byte = 1 << shift      // silent: default int, declared byte (nerdctl)
	var b int = 1 << shift       // reported
	var c byte = 1 << 2          // silent
	var d int = 1 << 2           // reported
	var e uint = 1 << shift      // silent
	var g int = 1 << (shift + 1) // reported
	var h uint8 = 3 >> shift     // silent
	var i int = shift2() << 1    // reported: a typed int on the left
	var j int = -1 << shift      // reported
	var k int64 = 1 << shift     // silent
	_, _, _, _, _, _, _, _, _, _ = a, b, c, d, e, g, h, i, j, k
}

func shift2() int { return 1 }
