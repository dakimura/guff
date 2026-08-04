package p

type Bad interface {
	Foo()
	foo()
}
