package p

func multi() (int, int, int, int) { return 1, 2, 3, 4 }

func Bad() {
	_, _, _, _ = multi()
}
