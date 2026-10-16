// Generates differential vectors for guff's port of go-internal/diff:
//
//	cd crates/guff-fmt/tests/testdata/godiff/gen && go run . > ../vectors.json
package main

import (
	"encoding/json"
	"math/rand"
	"os"
	"strings"

	"github.com/rogpeppe/go-internal/diff"
)

type vec struct {
	Old  string `json:"old"`
	New  string `json:"new"`
	Diff string `json:"diff"`
}

var alphabet = []string{"}", "", "\t}", "a", "b", "c", "func f() {", "\treturn", "x := 1", "import (", ")"}

func randLines(r *rand.Rand, n int) []string {
	out := make([]string, n)
	for i := range out {
		if r.Intn(4) == 0 {
			out[i] = alphabet[r.Intn(len(alphabet))]
		} else {
			out[i] = "line" + string(rune('A'+r.Intn(20)))
		}
	}
	return out
}

func mutate(r *rand.Rand, in []string) []string {
	out := append([]string(nil), in...)
	for k := r.Intn(6); k >= 0; k-- {
		if len(out) == 0 {
			out = append(out, "new")
			continue
		}
		i := r.Intn(len(out))
		switch r.Intn(4) {
		case 0:
			out = append(out[:i], out[i+1:]...)
		case 1:
			out = append(out[:i], append([]string{alphabet[r.Intn(len(alphabet))]}, out[i:]...)...)
		case 2:
			out[i] = out[i] + " "
		case 3:
			j := r.Intn(len(out))
			out[i], out[j] = out[j], out[i]
		}
	}
	return out
}

func join(l []string, nl bool) string {
	s := strings.Join(l, "\n")
	if nl && len(l) > 0 {
		s += "\n"
	}
	return s
}

func main() {
	r := rand.New(rand.NewSource(1))
	var vs []vec
	for i := 0; i < 600; i++ {
		a := randLines(r, r.Intn(40))
		b := mutate(r, a)
		old := join(a, r.Intn(8) != 0)
		new := join(b, r.Intn(8) != 0)
		vs = append(vs, vec{old, new, string(diff.Diff("a.go.orig", []byte(old), "a.go", []byte(new)))})
	}
	json.NewEncoder(os.Stdout).Encode(vs)
}
