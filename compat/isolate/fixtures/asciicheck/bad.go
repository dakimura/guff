package p

func Bad() {
	var Ä int // non-ASCII
	_ = Ä
}

// asciicheck names the identifier and the rune, so each non-ASCII declaration
// is its own sentence: a type, a func and a field are separate nodes.
type Ünicode struct {
	Naïve int
}

func Café() {}
