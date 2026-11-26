package dogsled

func ret3() (int, int, int) { return 1, 2, 3 }
func ret4() (int, int, int, int) { return 1, 2, 3, 4 }

func Bad() {
	_, _, _ = ret3()
	_, _, _, _ = ret4()
}
