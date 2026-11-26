package p

func f(x []string) {}

func BadCalls() {
	f([]string{"nested"})
	f([]string{"nested"})
	f([]string{"nested"})
}
