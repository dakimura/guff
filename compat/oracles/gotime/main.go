// Command gotime emits ground truth for guff's port of Go's time layout parser
// (crates/guff-staticcheck/src/gostd/time.rs, used by SA1002).
//
// It runs time.Parse(s, s) — exactly what staticcheck's SA1002 does, minus the
// `_`→` ` / `Z`→`-` substitutions the check applies first — over a deterministic
// corpus and prints one row per input:
//
//	<Go-quoted layout>	<hex of layout bytes>	<error, verbatim>
//
// An empty third column means Parse succeeded. The first column is for humans
// reading a diff; the second is what the test replays, because a layout may
// hold bytes that are not valid UTF-8. The error needs no encoding: time.quote
// escapes everything outside printable ASCII, so the message never contains a
// tab or a newline.
//
// Regenerate with ../regen.sh; never hand-edit the output.
package main

import (
	"bufio"
	"fmt"
	"os"
	"strconv"
	"time"
)

// stdTokens is every layout element nextStdChunk recognizes, plus the near
// misses that must *not* be recognized ("Janu", "Mond", "0000").
var stdTokens = []string{
	"2006", "06", "January", "Jan", "Janu", "Monday", "Mon", "Mond",
	"1", "01", "2", "02", "_2", "__2", "002", "_2006",
	"15", "3", "03", "4", "04", "5", "05",
	"PM", "pm", "MST", "0000",
	"Z0700", "Z07:00", "Z07", "Z070000", "Z07:00:00",
	"-0700", "-07:00", "-07", "-070000", "-07:00:00",
	".000", ".999", ",000", ",999", ".0000000000",
}

var separators = []string{"", "-", " ", ":", "T"}

// literals are layouts with few or no std elements, plus the byte-level corner
// cases `quote` has to render (non-ASCII, invalid UTF-8, control characters).
var literals = []string{
	"", "not-a-layout", "hello", "abc-def", "foo/bar", "T", "-", ":", " ",
	"a b", "  ", "Z", "_", "\t", "\n", "\x00", "\x1f", "\"", "\\",
	"é", "2006é", "\xff", "2006\xff", "\xef\xbf\xbd", "日本語",
	"UTC", "GMT", "GMT+3", "ChST", "WITA", "MSTT", "+03", "-24",
	// The staticcheck SA1002 fixture, and longer digit runs than the
	// exhaustive 1–3 digit sweep below reaches.
	"12345", "1234567", "20060102150405", "999999999999",
}

func corpus() []string {
	var out []string
	out = append(out, literals...)
	for _, l := range []string{
		time.Layout, time.ANSIC, time.UnixDate, time.RubyDate, time.RFC822,
		time.RFC822Z, time.RFC850, time.RFC1123, time.RFC1123Z, time.RFC3339,
		time.RFC3339Nano, time.Kitchen, time.Stamp, time.StampMilli,
		time.StampMicro, time.StampNano, time.DateTime, time.DateOnly,
		time.TimeOnly,
	} {
		out = append(out, l)
	}
	out = append(out, stdTokens...)
	for _, a := range stdTokens {
		for _, sep := range separators {
			for _, b := range stdTokens {
				out = append(out, a+sep+b)
			}
		}
	}
	// Every one-, two- and three-digit string, to cover getnum/getnum3.
	for n := 0; n < 1000; n++ {
		out = append(out, strconv.Itoa(n))
		if n < 100 {
			out = append(out, fmt.Sprintf("%02d", n))
		}
		if n < 10 {
			out = append(out, fmt.Sprintf("%03d", n))
		}
	}
	return out
}

func main() {
	w := bufio.NewWriter(os.Stdout)
	defer w.Flush()
	for _, s := range corpus() {
		msg := ""
		if _, err := time.Parse(s, s); err != nil {
			msg = err.Error()
		}
		fmt.Fprintf(w, "%s\t%x\t%s\n", strconv.Quote(s), s, msg)
	}
}
