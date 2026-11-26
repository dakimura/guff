package p

type T struct{ a, b int }

// go1.27 indentList: multi-line composite literals no longer count as
// multi-line elements, so this return list is not indented wholesale.
func pair() (T, T) {
	return T{
		a: 1,
	}, T{
		b: 2,
	}
}

func pairAddr() (*T, *T) {
	return &T{
		a: 1,
	}, &T{
		b: 2,
	}
}

// go1.27 intersperseComments: an unindented comment abutting an identifier
// is no longer reformatted as a doc comment.
func f() int {
	//   - indented list
	//nospace
	x := 1
	return x
}
