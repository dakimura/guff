package goto_ok

func scan() {
	pos := 0
	ch := next()
chomp:
	if ch == ' ' {
		ch = next()
		pos = 1
		goto chomp
	}
	_ = pos
}

func next() rune { return 'x' }
