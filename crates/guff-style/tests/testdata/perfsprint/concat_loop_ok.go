package p

func negative() {
	for w := 0; w < 10; w++ {
		s := "local"
		s = s + "y"
		_ = s
	}
	for w := 0; w < 10; w++ {
		var s string
		s = s + "y"
		_ = s
	}
	for w := 0; w < 10; w++ {
		var s2, s string
		s = s + "y"
		_ = s
		_ = s2
	}
	for w := 0; w < 10; w++ {
		s2, s := "local", "same"
		s = s + "y"
		_ = s
		_ = s2
	}
	nb := 0
	for w := 0; w < 10; w++ {
		nb += w
	}
	for w := 0; w < 10; w++ {
		nb = nb + w
	}
	words := []string{"one", "two", "three"}
	var s string
	for w := range words {
		s = "toto" + words[w]
	}
	var s2 string
	for w := range words {
		s = s2 + words[w]
	}
	// otherOps (len(s) / non-concat assign): skipped when loop-other-ops=false
	for w := 0; w < 10; w++ {
		s = s + "y"
		if len(s)%3 == 1 {
			s = s + ","
		}
	}
	for w := 0; w < 10; w++ {
		s = "reset"
		if w%2 == 1 {
			s = s + ","
		}
	}
	// Upstream's breadth-first walk leaves the loop body for exactly one kind
	// of statement:
	//
	//	case *ast.IfStmt:
	//		// explore breadth first, but go inside the if/else blocks
	//		if st.Body != nil { bl = append(bl, st.Body.List...) }
	//		el, ok := st.Else.(*ast.BlockStmt)
	//		if ok && el != nil { bl = append(bl, el.List...) }
	//
	// Everything below is a block it never enters. A nested loop is the one
	// that still gets reported, and not from here — `runConcatLoop` visits
	// every `ForStmt` / `RangeStmt` in the file, so the inner loop is a loop of
	// its own (see `concat_loop_bad.go`).
	//
	// guff used to walk into `switch` bodies, which is one finding in
	// telegraf's `migrations/inputs_udp_listener/migration.go:45` that
	// golangci-lint does not have. perfsprint gained the `switch` case in a
	// commit *after* v0.10.1, the version golangci-lint 2.12.2 pins.
	for w := 0; w < 10; w++ {
		switch w {
		case 1:
		default:
			s = s + "y"
		}
	}
	for w := 0; w < 10; w++ {
		switch w {
		case 1:
			s = s + "x"
		case 2:
			s = s + "y"
		}
	}
	var any1 interface{} = "x"
	for w := 0; w < 10; w++ {
		switch v := any1.(type) {
		case string:
			s = s + v
		}
		_ = w
	}
	ch := make(chan string, 1)
	for w := 0; w < 10; w++ {
		select {
		case v := <-ch:
			s = s + v
		default:
		}
		_ = w
	}
	for w := 0; w < 10; w++ {
		{
			s = s + "y"
		}
		_ = w
	}
	// `st.Else` is an `*ast.IfStmt` here, not a `*ast.BlockStmt`, so the walk
	// stops at the `else`.
	for w := 0; w < 10; w++ {
		if w%3 == 0 {
		} else if w%3 == 1 {
			s = s + "y"
		}
	}
	_ = s
	_ = s2
	_ = nb
}
