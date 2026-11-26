package fileignore

// A method of the ignored type, written in a file the directive does not cover.
func (c *cluster) inPlainFile() int { return c.n }

// Reached only from the ignored file, and only because an ignored object is a
// root there.
func keptAlive() int { return 2 }

// The negative control: a type declared here keeps its dead method reported.
type plain struct{ n int }

func (p *plain) unusedMethod() int { return p.n }

func unusedFree() int { return 3 }

// Exported entry point, so `plain` itself is alive and only its method is dead.
func Run() int { return (&plain{n: 1}).n }
