package dupltest

func alpha() {
	a := 1
	b := 2
	c := a + b
	if c > 0 {
		d := c * 2
		e := d + 1
		f := e - 1
		g := f * 3
		h := g / 2
		_ = h
	}
	for i := 0; i < 10; i++ {
		j := i * 2
		k := j + 1
		l := k - 1
		m := l * 3
		n := m / 2
		_ = n
	}
	switch c {
	case 1:
		x := a + 1
		_ = x
	case 2:
		y := b + 2
		_ = y
	default:
		z := c + 3
		_ = z
	}
}

func beta() {
	a := 1
	b := 2
	c := a + b
	if c > 0 {
		d := c * 2
		e := d + 1
		f := e - 1
		g := f * 3
		h := g / 2
		_ = h
	}
	for i := 0; i < 10; i++ {
		j := i * 2
		k := j + 1
		l := k - 1
		m := l * 3
		n := m / 2
		_ = n
	}
	switch c {
	case 1:
		x := a + 1
		_ = x
	case 2:
		y := b + 2
		_ = y
	default:
		z := c + 3
		_ = z
	}
}
