package utf8

func RuneCount(p []byte) int              { return 0 }
func RuneCountInString(s string) int      { return 0 }
func Valid(p []byte) bool                 { return true }
func ValidString(s string) bool           { return true }
func FullRune(p []byte) bool              { return true }
func FullRuneInString(s string) bool      { return true }
func DecodeRune(p []byte) (rune, int)     { return 0, 0 }
func DecodeRuneInString(s string) (rune, int) { return 0, 0 }
func DecodeLastRune(p []byte) (rune, int) { return 0, 0 }
func DecodeLastRuneInString(s string) (rune, int) { return 0, 0 }
