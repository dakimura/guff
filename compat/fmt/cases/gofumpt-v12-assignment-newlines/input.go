package p

func f() {
    foo :=


        "bar"

    foo :=
        "bar"

	_, _ =
		0,
		1

	_, _ = 0,
		1

    _ =
        `
            foo
        `

	_ = /* inline */
		"foo"

	_ = // inline
		"foo"

	_ = /* multi
		line */
		"foo"

	long :=
		"foo " +
			"bar " +
			"baz"
	_ = long

	a, b, c :=
		"foo",
		"bar",
		"qux"
	_, _, _ = a, b, c
}

