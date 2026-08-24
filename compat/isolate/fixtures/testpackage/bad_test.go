package p

import "testing"

func TestBad(t *testing.T) {}

// testpackage reports each test file whose package is not `_test`, so a second
// file is a second finding — and the message names the file.
func TestAlso(t *testing.T) {}

func BenchmarkBad(b *testing.B) {}
