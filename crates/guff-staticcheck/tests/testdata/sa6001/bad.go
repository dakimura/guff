package main
func f(m map[string]int, b []byte) {
  k := string(b)
  _ = m[k]
  _ = m[k]
}
