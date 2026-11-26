package main

import (
	htmltemplate "html/template"
	"text/template"
)

func main() {
	// SA1001 reports only the parse errors whose text contains "unexpected" or
	// "bad character" — one call per shape that produces one.
	template.New("").Parse("{{.Name}} {{.LastName}")
	template.New("").Parse("{{x+y}}")
	template.New("").Parse("{{)}}")
	template.New("").Parse("{{if .}}")
	template.New("").Parse("{{end}}")
	template.New("").Parse("{{,}}")
	template.New("").Parse("{{true.x}}")
	template.New("").Parse("{{template 1}}")
	template.New("").Parse("{{1.2.3}}")
	template.New("").Parse(`{{define "d"}}{{else}}{{end}}`)
	// The line in the message is the offending token's, not the call's.
	template.New("").Parse("line one\n{{end}}")
	// html/template.Parse hands the text to text/template, so it fails with
	// exactly the same error.
	htmltemplate.New("").Parse("{{end}}")
}
