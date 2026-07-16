package example

// Big declares more than the default 10 methods and should be flagged.
type Big interface {
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
	M11()
}
