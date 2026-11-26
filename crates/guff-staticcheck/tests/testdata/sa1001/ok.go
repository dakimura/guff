package main

import "text/template"

func main() {
	template.New("").Parse("{{.Name}}")
	template.New("").Parse("{{if .}}{{end}}")
	template.New("").Parse("{{range $i, $v := .}}{{$i}}{{end}}")
	template.New("").Parse("{{/* c */}}")
	template.New("").Parse("{{- .x -}}")
	template.New("").Parse(`{{printf "%d" 1}}`)

	// Templates that fail to parse, but with errors outside the two classes
	// SA1001 keeps. Stopping at a *different* error than Go does would surface
	// here as a report, so these are the negative half of the port's gate.
	template.New("").Parse("{{undefinedfn}}")
	template.New("").Parse("{{1e}}")
	template.New("").Parse("{{$x}}")
	template.New("").Parse(`{{"a}}`)
	template.New("").Parse("{{/* c}}")
	template.New("").Parse("{{")
	template.New("").Parse("{{'ab'}}")
	template.New("").Parse("{{print .x | 1}}")

	// Not a New(...) receiver. Upstream's workaround for templates with custom
	// delimiters skips any receiver it cannot see through, invalid or not.
	t := template.New("x")
	t.Parse("{{.Name}} {{.LastName}")
}
