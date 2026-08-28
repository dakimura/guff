//go:build ignore

// Reference side of the `go/doc/comment` differential in
// crates/guff-ast/tests/doc_comment_differential.rs.
//
// Two modes, both speaking lowercase hex, one record per line:
//
//	go run scripts/doccomment-differential.go -extract DIR...
//	    Emits the *comment text* (markers stripped the way go/printer's
//	    formatDocComment strips them) of every `//`-group and every multiline
//	    `/* */` in the .go files under DIR, deduplicated.
//
//	go run scripts/doccomment-differential.go < inputs
//	    Reads those texts and emits, for each, what a zero-value
//	    comment.Parser + comment.Printer.Comment produces — the exact round
//	    trip gofmt performs on a doc comment.
//
// Hex rather than base64 so the Rust side needs no dependency, and one record
// per line so a mismatch names an index the driver can print in full.
package main

import (
	"bufio"
	"encoding/hex"
	"fmt"
	"go/ast"
	"go/doc/comment"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"strings"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "-extract" {
		extract(os.Args[2:])
		return
	}
	convert()
}

func convert() {
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 1<<20), 1<<24)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	var p comment.Parser
	var pr comment.Printer
	for in.Scan() {
		raw, err := hex.DecodeString(in.Text())
		if err != nil {
			fmt.Fprintln(os.Stderr, "bad input record:", err)
			os.Exit(1)
		}
		fmt.Fprintln(out, hex.EncodeToString(pr.Comment(p.Parse(string(raw)))))
	}
	if err := in.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "read:", err)
		os.Exit(1)
	}
}

func extract(roots []string) {
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	seen := map[string]bool{}
	for _, root := range roots {
		_ = filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
			if err != nil || info.IsDir() || !strings.HasSuffix(path, ".go") {
				return nil
			}
			fset := token.NewFileSet()
			f, err := parser.ParseFile(fset, path, nil, parser.ParseComments|parser.SkipObjectResolution)
			if err != nil {
				return nil // deliberately malformed testdata; not our problem
			}
			for _, cg := range f.Comments {
				t, ok := commentText(cg)
				if !ok || seen[t] {
					continue
				}
				seen[t] = true
				fmt.Fprintln(out, hex.EncodeToString([]byte(t)))
			}
			return nil
		})
	}
}

// commentText mirrors the marker-stripping half of go/printer's
// formatDocComment, so the differential feeds the parser exactly what gofmt
// feeds it — including the directive lines it holds back.
func commentText(cg *ast.CommentGroup) (string, bool) {
	if len(cg.List) == 1 && strings.HasPrefix(cg.List[0].Text, "/*") {
		t := cg.List[0].Text
		if !strings.Contains(t, "\n") {
			return "", false // single-line block comment: formatDocComment bails
		}
		return t[2 : len(t)-2], true
	}
	var b strings.Builder
	for _, c := range cg.List {
		after, found := strings.CutPrefix(c.Text, "//")
		if !found {
			return "", false
		}
		if isDirective(after) {
			continue
		}
		b.WriteString(strings.TrimPrefix(after, " "))
		b.WriteString("\n")
	}
	return b.String(), b.Len() > 0
}

// isDirective is go/printer's copy, reproduced here because it is not exported.
func isDirective(c string) bool {
	if strings.HasPrefix(c, "line ") || strings.HasPrefix(c, "extern ") || strings.HasPrefix(c, "export ") {
		return true
	}
	colon := strings.Index(c, ":")
	if colon <= 0 || colon+1 >= len(c) {
		return false
	}
	for i := 0; i <= colon+1; i++ {
		if i == colon {
			continue
		}
		b := c[i]
		if !('a' <= b && b <= 'z' || '0' <= b && b <= '9') {
			return false
		}
	}
	return true
}
