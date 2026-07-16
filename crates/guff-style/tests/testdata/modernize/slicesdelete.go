package modernize

var g struct{ f []int }

func h() []int { return []int{} }

var ch chan []int

func slicesdelete(test, other []byte, i int) {
	const k = 1
	_ = append(test[:i], test[i+1:]...) // want

	_ = append(test[:i+1], test[i+2:]...) // want

	_ = append(test[:i+1], test[i+1:]...) // not deleting

	_ = append(test[:i], test[i-1:]...) // not deleting

	_ = append(test[:i-1], test[i:]...) // want

	_ = append(test[:i-2], test[i+1:]...) // want

	_ = append(test[:i-2], other[i+1:]...) // different slices

	_ = append(test[:i-2], other[i+1+k:]...) // cannot verify

	_ = append(test[:i-2], test[11:]...) // cannot verify

	_ = append(test[:1], test[3:]...) // want

	_ = append(g.f[:i], g.f[i+k:]...) // want

	_ = append(h()[:i], h()[i+1:]...) // side effects

	_ = append((<-ch)[:i], (<-ch)[i+1:]...) // side effects

	_ = append(test[:3], test[i+1:]...) // cannot verify

	_ = append(test[:i-4], test[i-1:]...) // want

	_ = append(test[:1+2], test[3+4:]...) // want

	_ = append(test[:1+2], test[i-1:]...) // cannot verify
}

func alreadyDelete(s []int, i int) []int {
	return slicesDelete(s, i, i+1)
}

// Stand-in so ok.go-style code does not need the slices stub for this file.
func slicesDelete(s []int, i, j int) []int { return s }
