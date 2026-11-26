package v5

type T struct {
	A int
	//exhaustruct:optional
	B int
	_ int
}

//exhaustruct:ignore
type Skipped struct{ A int }

type Tagged struct {
	A int `exhaustruct:"optional"`
}

func f() {
	_ = T{}       // want "v5.T is missing field A"
	_ = T{A: 1}   // B is optional, and `_` has no name to write
	_ = Skipped{} // the type ignores itself
	_ = Tagged{}  // want "v5.Tagged is missing field A": the tag is inert in v5

	//exhaustruct:ignore
	_ = T{}
}
