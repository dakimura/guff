package flag

func Bool(name string, value bool, usage string) *bool { return &value }
func Duration(name string, value int, usage string) *int { return &value }
func Float64(name string, value float64, usage string) *float64 { return &value }
func Int(name string, value int, usage string) *int { return &value }
func Int64(name string, value int64, usage string) *int64 { return &value }
func String(name string, value string, usage string) *string { return &value }
func Uint(name string, value uint, usage string) *uint { return &value }
func Uint64(name string, value uint64, usage string) *uint64 { return &value }
