package main

import "bytes"

func f(buf bytes.Buffer) string { return buf.String() }

// `m[string(buf.Bytes())]` is exempt: a compiler optimization makes it faster
// than `m[buf.String()]`, so upstream skips a conversion whose parent node is
// an IndexExpr.
func lookup(m map[string]int, buf bytes.Buffer) int { return m[string(buf.Bytes())] }

type notBuffer struct{}

func (notBuffer) Bytes() []byte { return nil }

func other(n notBuffer) string { return string(n.Bytes()) }
