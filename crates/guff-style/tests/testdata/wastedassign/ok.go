package wastedassign_ok

import "fmt"

func usedAfterAssign() {
	a := 0
	fmt.Print(a)
	a = 1
	fmt.Print(a)
}

func usedBetweenReassign() {
	b := 0
	fmt.Print(b)
	b = 1
	fmt.Print(b)
	b = 2
	fmt.Print(b)
}

type storage interface{ List() int }

func loadStorage(useCustom bool) (storage, error) {
	return nil, nil
}

// If/else both assign `stor`, then shared use after merge (caddy storagefuncs).
func okIfElseAssignThenUse(useCustom bool) (int, error) {
	var stor storage
	var err error
	if useCustom {
		stor, err = loadStorage(true)
		if err != nil {
			return 0, err
		}
	} else {
		stor, err = loadStorage(false)
		if err != nil {
			return 0, err
		}
	}
	return stor.List(), nil
}

type fileInfo struct{}

func (fileInfo) IsDir() bool { return true }

func stat(_ string) (fileInfo, error) { return fileInfo{}, nil }

// If-init assign used only in Cond (caddy mkdirAllInherit / filewriter).
func okIfInitUsedInCond(dir string) bool {
	if fi, err := stat(dir); err == nil && fi.IsDir() {
		return true
	}
	return false
}

// Type assert local used via method call (caddy acmeWrapper pattern).
type acmeCapable interface{ GetACMEIssuer() int }

func okTypeAssertUse(issuer any) int {
	acmeWrapper, ok := issuer.(acmeCapable)
	if !ok {
		return 0
	}
	return acmeWrapper.GetACMEIssuer()
}

// Post-decrement whose value is still live for the loop (caddy i-- pattern).
func okIncDecReuse(xs []int) int {
	n := 0
outer:
	for i := 0; i < len(xs); i++ {
		for j := i + 1; j < len(xs); j++ {
			if xs[i] == xs[j] {
				i--
				continue outer
			}
		}
		n++
	}
	return n
}

// Integer range (Go 1.22+) uses a synthetic SSA local `rangeint.iter`.
func okIntegerRange(n int) int {
	sum := 0
	for range n {
		sum++
	}
	return sum
}
