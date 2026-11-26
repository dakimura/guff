package strings

func Compare(a, b string) int                          { return 0 }
func Contains(s, substr string) bool                   { return false }
func ContainsAny(s, chars string) bool                 { return false }
func ContainsRune(s string, r rune) bool               { return false }
func Count(s, substr string) int                       { return 0 }
func EqualFold(s, t string) bool                       { return false }
func HasPrefix(s, prefix string) bool                  { return false }
func HasSuffix(s, suffix string) bool                  { return false }
func Index(s, substr string) int                       { return -1 }
func IndexAny(s, chars string) int                     { return -1 }
func IndexByte(s string, c byte) int                   { return -1 }
func IndexFunc(s string, f func(rune) bool) int        { return -1 }
func IndexRune(s string, r rune) int                   { return -1 }
func LastIndex(s, substr string) int                   { return -1 }
func LastIndexAny(s, chars string) int                 { return -1 }
func LastIndexByte(s string, c byte) int               { return -1 }
func LastIndexFunc(s string, f func(rune) bool) int    { return -1 }

type Builder struct{}

func (b *Builder) Write(p []byte) (int, error)       { return 0, nil }
func (b *Builder) WriteString(s string) (int, error) { return 0, nil }
func (b *Builder) WriteRune(r rune) (int, error)     { return 0, nil }
