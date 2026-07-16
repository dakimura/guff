package p

func positive() {
	var s string
	words := []string{"one", "two", "three"}
	for w := range words {
		s += words[w]
	}
	for w := range words {
		s = s + words[w]
	}
	for w := 0; w < 10; w++ {
		s = s + "y"
	}
	for w := 0; w < 10; w++ {
		if w%2 == 1 {
			s = s + "y"
		}
	}
	nb := 0
	for w := 0; w < 10; w++ {
		if w%2 == 1 {
			nb += 1
		} else {
			s = s + "y"
		}
	}
	for w := 0; w < 10; w++ {
		if w%2 == 1 {
			s = s + "x"
		} else {
			s = s + "y"
		}
	}
	s2 := "prefix"
	for w := 0; w < 10; w++ {
		if w%2 == 1 {
			s2 = s2 + "x"
		} else {
			s = s + "y"
		}
	}
	for w := 0; w < 10; w++ {
		for y := 0; y < 10; y++ {
			s = s + "a"
		}
		s = s + ","
	}
	for w := 0; w < 10; w++ {
		switch w {
		case 1:
		default:
			s = s + "y"
		}
	}
	_ = nb
	_ = s2
}
