//go:build go1.23

package modernize

func sumBackward(s []int) int {
	sum := 0
	for i := len(s) - 1; i >= 0; i-- {
		sum += s[i]
	}
	return sum
}
