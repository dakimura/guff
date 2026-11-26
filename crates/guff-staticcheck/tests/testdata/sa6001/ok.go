package main

func f(m map[string]int, b []byte) { _ = m[string(b)] }

// A map *write* is an `ir.MapUpdate`, not an `ir.MapLookup`, so it lands on
// upstream's `default:` arm and abandons the conversion. gitea writes
// `attributesMap[filename] = attribute2info` beside its read.
func write(m map[string]int, b []byte) int {
	k := string(b)
	v := m[k]
	m[k] = v + 1
	return v
}

// So does any other use of the key — argo-cd interpolates its into an error
// message next to the lookup.
func otherUse(m map[string]int, b []byte) (int, string) {
	k := string(b)
	v := m[k]
	return v, k
}

// `m[k]++` reads and writes.
func incdec(m map[string]int, b []byte) {
	k := string(b)
	m[k]++
}
