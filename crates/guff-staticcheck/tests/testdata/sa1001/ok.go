package main

import "text/template"

const tmpl1 = `{{.Name}}`

func main() {
	template.New("").Parse(tmpl1)
}
