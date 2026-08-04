package p

type A struct{ X int }
type B struct{ Y int }

type Bad struct {
	A
	Z int
	B
}
