package exhaustruct

type Point struct {
	X int
	Y int
	Z int `exhaustruct:"optional"`
}

func bad() {
	_ = Point{ // want missing Y
		X: 1,
	}
	_ = Point{} // want missing X, Y
	_ = struct { // want missing B
		A string
		B int
	}{
		A: "a",
	}
}
