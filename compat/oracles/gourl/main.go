// Command gourl emits ground truth for guff's port of net/url.Parse
// (crates/guff-staticcheck/src/gostd/url.rs, used by SA1007).
//
// It runs url.Parse over a deterministic corpus and prints one row per input:
//
//	<Go-quoted input>	<hex of input bytes>	<error, verbatim>
//
// An empty third column means Parse succeeded. SA1007 wraps the error as
// `%q is not a valid URL: %s`, so the raw error is what the port must match.
//
// go.mod pins `go 1.25.0` on purpose. Go 1.26 added the `urlstrictcolons`
// godebug (go.dev/issue/75223), whose default is derived from the main module's
// go directive: under 1.26+ an http/https host takes the *first* colon as the
// port separator, so "http://h1:5432:5433/" becomes an error, and under 1.25
// and earlier it takes the last and parses clean. golangci-lint v2.12.2
// declares `go 1.25.0`, so that is the behaviour guff has to match. Bump this
// directive when golangci-lint bumps its own, and expect the golden to move.
//
// Regenerate with ../regen.sh; never hand-edit the output.
package main

import (
	"bufio"
	"fmt"
	"net/url"
	"os"
	"strconv"
	"strings"
)

// pieces are assembled into every combination below. They are chosen to hit
// each error return in net/url.parse, parseAuthority, parseHost and unescape:
// the missing scheme, the colon in a relative first segment, control bytes,
// bad %-escapes, bad ports, bracket handling, and userinfo.
var (
	schemes = []string{
		"", "http:", "https:", "ftp:", "HTTP:", "a+b-c.d:", "1http:", "+:", ":",
	}
	authorities = []string{
		"", "//", "///", "//host", "//host:80", "//host:", "//host:port",
		"//host:80:90", "//h o st", "//user@host", "//user:pass@host",
		"//us er@host", "//user@name@host", "//%41@host", "//%zz@host",
		"//[::1]", "//[::1]:80", "//[::1]:x", "//[::1", "//x[::1]", "//[]",
		"//[1.2.3.4]", "//[::ffff:1.2.3.4]", "//[fe80::1%25en0]",
		"//[fe80::1%en0]", "//[:::]", "//[12345::]", "//[::1]extra",
		"//%2f", "//h%2fst", "//h<st", "//h\"st", "//h|st", "//h{st",
	}
	tails = []string{
		"", "/", "/path", "/path/to", "/pa th", "/%2f", "/%zz", "/%",
		"?", "?a=b", "?a=b?c=d", "#", "#frag", "#%zz", "#fr ag",
		"/p?q#f", ":opaque", "rel/path", "a:b/c", "./rel", "../rel",
	}
)

func corpus() []string {
	seen := map[string]bool{}
	var out []string
	add := func(s string) {
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	for _, s := range schemes {
		for _, a := range authorities {
			for _, t := range tails {
				add(s + a + t)
			}
		}
	}
	// Standalone shapes that the grid cannot express.
	for _, s := range []string{
		"*", "foobar", "foo bar", "cache_object:foo/bar", "mailto:a@b.c",
		"//", "/", ".", "..", "%", "%2", "%zz", "%25", "%41",
		"http://a\x00b", "http://a\nb", "ht\x7ftp://x", "\x1f",
		"http://[fe80::1%25%32%35en0]", "http://host/#a#b",
		"postgres://h1:5432,h2:5433/db", "http://h1:5432,h2:5433/db",
		"scheme://h1:5432:5433/", "http://h1:5432:5433/",
		"http://\u00e9.example.com/", "http://example.com/\u00e9",
	} {
		add(s)
	}
	return out
}

func main() {
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	for _, s := range corpus() {
		msg := ""
		if _, err := url.Parse(s); err != nil {
			msg = err.Error()
			if strings.ContainsAny(msg, "\t\n") {
				// The Rust side reads a tab-separated line; a message that
				// carried either would silently truncate the expectation.
				fmt.Fprintf(os.Stderr, "error text contains a separator for %q: %q\n", s, msg)
				os.Exit(1)
			}
		}
		fmt.Fprintf(w, "%s\t%x\t%s\n", strconv.Quote(s), s, msg)
	}
}
