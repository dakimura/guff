package pkg

import "fmt"

type StringWriter interface {
	WriteString(string) (int, error)
}

func fn() {
	var sw StringWriter
	// StringWriter only — no Write method, so not flagged.
	sw.WriteString(fmt.Sprint("abc", "de"))
}
