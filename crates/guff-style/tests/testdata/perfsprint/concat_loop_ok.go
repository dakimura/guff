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
	_ = s
	_ = s2
	_ = nb
}
