package main

func maybe() *int { return nil }

func main() {
	// A pointer that really can be nil.
	var p *int
	if p == nil {
		return
	}
	// Returned values are not provably non-nil.
	q := maybe()
	if q == nil {
		return
	}
	// Upstream only walks `if` conditions: a bare comparison is not a finding,
	// even when the operand is a fresh channel.
	_ = make(chan int) == nil
	// `&x == nil` belongs to SA4022, which SA4031 skips so they do not both fire.
	var x int
	if &x == nil {
		return
	}
}
