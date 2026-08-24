package p

type Bad interface {
	Do(int, string)
}

// inamedparam names the interface and the method, so each unnamed-parameter
// method is its own sentence.
type Multi interface {
	One(int)
	Two(string, bool)
}

// A method whose parameters are all named is the negative half.
type Named interface {
	Ok(n int, s string)
}
