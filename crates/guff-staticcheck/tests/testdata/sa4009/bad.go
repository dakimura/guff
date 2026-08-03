package main

func f(arg int) {
	arg = 1
}

// Param overwritten before first use; later uses of the same ObjectId must not suppress.
func withTimeout(ctx interface{}, host string) {
	ctx, _ = identity(nil)
	_ = ctx
	_ = host
}

func identity(x interface{}) (interface{}, error) {
	return x, nil
}

func main() {
	f(0)
	withTimeout(nil, "")
}
