package p

type Bad interface {
	A()
	B()
	C()
	D()
	E()
	F()
	G()
	H()
	I()
	J()
	K()
	L()
}

// The message counts the methods, so a different count is a different
// sentence. An embedded interface counts as **one** toward the total, not as
// the methods it brings — `Small` plus nine here is eleven, and dropping one
// would put this at ten and report nothing.
type Small interface {
	A()
	B()
}

type ViaEmbedding interface {
	Small
	C()
	D()
	E()
	F()
	G()
	H()
	I()
	J()
	K()
	L()
}
