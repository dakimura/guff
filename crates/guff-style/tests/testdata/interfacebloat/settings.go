package example

// Three declares three methods: allowed by default (max 10) but flagged
// when the configured max is lowered to 2.
type Three interface {
	A()
	B()
	C()
}
