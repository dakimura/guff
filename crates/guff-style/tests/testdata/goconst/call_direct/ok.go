package p

func f(x string) {}

func OkDirectCallArgs() {
	// With ignore-calls (golangci default), direct string args are ignored.
	f("direct")
	f("direct")
	f("direct")
}
