package main

func f() int {
	switch 1 {
	case 1:
		println(1)
	}
	return 1
}

func g() int {
	return 1
}

func h() {
outer:
	for {
		switch 1 {
		case 1:
			break outer
		}
	}
}
