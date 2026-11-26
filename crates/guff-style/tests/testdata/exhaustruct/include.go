package include

type Included struct {
	A string
	B int
}

type Other struct {
	A string
	B int
}

func f() {
	_ = Included{} // flagged when include matches Included
	_ = Other{}    // skipped when not in include
}
