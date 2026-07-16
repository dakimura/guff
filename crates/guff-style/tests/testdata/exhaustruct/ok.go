package exhaustruct

type Point struct {
	X int
	Y int
	Z int `exhaustruct:"optional"`
}

func ok() {
	_ = Point{
		X: 1,
		Y: 2,
	}
	_ = Point{
		X: 1,
		Y: 2,
		Z: 3,
	}
	_ = Point{1, 2, 3}
	_ = &Point{
		X: 1,
		Y: 2,
	}
	_ = struct {
		A string
		B int
	}{
		A: "a",
		B: 1,
	}
}

type myErr struct{ msg string }

func (e myErr) Error() string { return e.msg }

func okErrorReturn() (Point, error) {
	return Point{}, myErr{msg: "boom"}
}
