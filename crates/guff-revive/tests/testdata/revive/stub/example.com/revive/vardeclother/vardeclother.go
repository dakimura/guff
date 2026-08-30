// Package vardeclother is the "another package" half of the var-declaration
// cross-package gate. It exists so the fixture can reach into an import in
// every way that has no package qualifier to notice.
package vardeclother

type Case struct{ Name string }

type Box struct{ S string }

func (b Box) Method() string { return b.S }

const Answer = 42

func TestFunc(c *Case) func() { return func() {} }

func Str() string { return "" }
