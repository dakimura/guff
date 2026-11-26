package atomic

func AddInt64(addr *int64, delta int64) int64 {
	var v int64
	return v
}

func AddUint64(addr *uint64, delta uint64) uint64 {
	var v uint64
	return v
}

func CompareAndSwapInt64(addr *int64, old, new int64) bool {
	return false
}

func CompareAndSwapUint64(addr *uint64, old, new uint64) bool {
	return false
}

func LoadInt64(addr *int64) int64 {
	var v int64
	return v
}

func LoadUint64(addr *uint64) uint64 {
	var v uint64
	return v
}

func StoreInt64(addr *int64, val int64) {}

func StoreUint64(addr *uint64, val uint64) {}

func SwapInt64(addr *int64, new int64) int64 {
	var v int64
	return v
}

func SwapUint64(addr *uint64, new uint64) uint64 {
	var v uint64
	return v
}
