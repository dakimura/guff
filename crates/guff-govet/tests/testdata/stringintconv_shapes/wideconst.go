//go:build go1.21

package extra

func _() {
	println(string(int64(1 << 40))) // typed const past int32: %c + %d conflict
}
