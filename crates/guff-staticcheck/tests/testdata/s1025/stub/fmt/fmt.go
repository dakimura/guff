package fmt

func Sprintf(format string, a ...interface{}) string { return "" }

// Stringer is what S1025's first branch tests against —
// `types.Implements(typ, knowledge.Interfaces["fmt.Stringer"])`.
type Stringer interface {
	String() string
}

// State is the first parameter of a Format method. `isFormatter` does not look
// at the parameter types (upstream has a TODO saying so), but the fixture needs
// a type to name.
type State interface {
	Write(b []byte) (n int, err error)
}
