package example

// Small has exactly 10 methods, which is the default limit (not more than).
type Small interface {
	M1()
	M2()
	M3()
	M4()
	M5()
	M6()
	M7()
	M8()
	M9()
	M10()
}

// Empty is the common any-like alias and must never be flagged.
type Empty interface{}
