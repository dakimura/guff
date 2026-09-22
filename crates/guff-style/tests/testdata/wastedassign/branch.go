package wastedassign_branch

// A store in a block that ends in `break` or `continue`, with a mention of
// the variable later in the text. guff's AST fallback (which excuses stores
// its NaiveForm SSA calls wasted but go/ssa would not) is positional, and a
// later mention kept the store "live" even when the branch jumps over it —
// beats' packetbeat/protos/mysql row loop (shape D). The fallback now skips
// what a branch jumps over: to the end of a `break`'s target, to the end of
// a `continue`d loop's body.
//
// Measured against golangci-lint 2.12.2: D E F H J L are reported; G (read on
// the next iteration), I (the break only leaves the switch) and K (read after
// the loop) are silent.

type lenErr struct{}

func (lenErr) Error() string { return "x" }

func readLength(d []byte, o int) (int, error) {
	if o >= len(d) {
		return 0, lenErr{}
	}
	return int(d[o]), nil
}

// D: the loop reads an outer `length`, reassigned with `, err =`
func D(data []byte) (n int) {
	length, err := readLength(data, 0)
	if err != nil {
		return 0
	}
	offset := 5
	for offset < len(data) {
		if data[offset+4] == 0xfe {
			offset += length + 4
			break
		}
		length, err = readLength(data, offset)
		if err != nil {
			break
		}
		n++
		offset += length + 4
	}
	return n
}

// E: same with a fresh `l :=` per iteration (matched before)
func E(data []byte) (n int) {
	offset := 5
	for offset < len(data) {
		l, err := readLength(data, offset)
		if err != nil {
			break
		}
		if data[offset+4] == 0xfe {
			offset += l + 4
			break
		}
		n++
		offset += l + 4
	}
	return n
}

// F: outer `length` but plain `=` with no err
func F(data []byte) (n int) {
	length := 0
	offset := 5
	for offset < len(data) {
		if data[offset+4] == 0xfe {
			offset += length + 4
			break
		}
		length = int(data[offset])
		n++
		offset += length + 4
	}
	return n
}

// G: continue — the rest of the body is unreachable, the next iteration reads it
func G(data []byte) (n int) {
	x := 0
	for i := 0; i < len(data); i++ {
		if data[i] == 0 {
			x = i
			continue
		}
		n += x
	}
	return n
}

// H: continue, and the value is overwritten before any read on the next pass
func H(data []byte) (n int) {
	for i := 0; i < len(data); i++ {
		x := 0
		if data[i] == 0 {
			x = i
			continue
		}
		n += x
	}
	return n
}

// I: break out of a switch inside a loop: the code after the switch runs
func I(data []byte) (n int) {
	x := 0
	for i := 0; i < len(data); i++ {
		switch data[i] {
		case 0:
			x = i
			break
		}
		n += x
	}
	return n
}

// J: labeled break to the outer loop: the rest of the outer body is skipped
func J(data [][]byte) (n int) {
	x := 0
outer:
	for _, row := range data {
		for _, b := range row {
			if b == 0 {
				x = int(b)
				break outer
			}
		}
		n += x
	}
	return n
}

// K: break out of the loop, and the value is read after the loop
func K(data []byte) (n int) {
	x := 0
	for i := 0; i < len(data); i++ {
		if data[i] == 0 {
			x = i
			break
		}
		n += x
	}
	return n + x
}

// L: the break-block is inside a nested if; a read later in the body is skipped
func L(data []byte) (n int) {
	x := 0
	for i := 0; i < len(data); i++ {
		if data[i] == 0 {
			if n > 1 {
				x = i
				break
			}
		}
		n += x
	}
	return n
}
