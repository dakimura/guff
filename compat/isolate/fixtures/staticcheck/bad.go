package p

import (
	"fmt"
	"strings"
)

// S1003
func BadIndex(s string) bool {
	return strings.Index(s, "x") == -1
}

// SA4017 + S1039
func DiscardedSprintf() {
	fmt.Sprintf("unused result")
}

func UsedSprintf() string {
	return fmt.Sprintf("hello")
}

// S1009: should omit nil check before len
func NilLen(s []int) bool {
	return s != nil && len(s) != 0
}

// SA4023: typed nil stored in interface, then compared to nil
func TypedNilIface() bool {
	var p *int
	var i any
	i = p
	return i == nil
}
