// Package sortableok skips Len/Less/Swap docs on sort.Interface types.
package sortableok

// ItemList is a sortable list.
type ItemList []int

func (s ItemList) Len() int           { return len(s) }
func (s ItemList) Less(i, j int) bool { return s[i] < s[j] }
func (s ItemList) Swap(i, j int)      { s[i], s[j] = s[j], s[i] }
