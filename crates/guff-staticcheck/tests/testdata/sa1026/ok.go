package main

import "encoding/json"

type Safe struct {
	A int
	B string
}

func main() {
	json.Marshal(Safe{A: 1, B: "ok"})
}

// Everything below holds something unmarshalable and is still silent, because
// the type marshals itself — or, for a map, because its key does.

// TextKey is moby's `network.Port`: a struct key with a value-receiver
// MarshalText. telegraf marshals a `map[Port][]PortBinding` and guff reported
// it.
type TextKey struct{ N int }

func (k TextKey) MarshalText() ([]byte, error) { return nil, nil }

type JSONer struct{ C chan int }

func (j JSONer) MarshalJSON() ([]byte, error) { return nil, nil }

type PtrJSONerOK struct{ C chan int }

func (j *PtrJSONerOK) MarshalJSON() ([]byte, error) { return nil, nil }

type Texter struct{ C chan int }

func (t Texter) MarshalText() ([]byte, error) { return nil, nil }

type wrapsTextKey struct {
	M map[TextKey][]int
}

func silent(a map[TextKey][]int, b JSONer, c *PtrJSONerOK, d Texter, e wrapsTextKey) {
	json.Marshal(a)
	json.Marshal(b)
	// `*PtrJSONerOK` implements it directly — no CanAddr needed.
	json.Marshal(c)
	json.Marshal(d)
	json.Marshal(e)
}
