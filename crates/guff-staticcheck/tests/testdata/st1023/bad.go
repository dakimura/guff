package pkg

type MyInt int

const Y = 1

func gen1() int   { return 0 }
func gen3() MyInt { return 0 }

var global int = gen1() // package-level: not flagged

func fn() {
	var _ int = gen1()
	var a int = Y
	var b int = 1
	var c int = 1.0
	var d MyInt = 1
	var h bool = true
	var i string = ""
	var j MyInt = gen3()
	var m int = (Y + Y) / 2
	_, _, _, _, _, _, _, _ = a, b, c, d, h, i, j, m
}
