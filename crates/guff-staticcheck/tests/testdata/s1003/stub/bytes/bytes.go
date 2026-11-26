package bytes

func Index(s, sep []byte) int { return -1 }
func IndexRune(s []byte, r rune) int { return -1 }
func IndexAny(s []byte, chars string) int { return -1 }
func Contains(s, sep []byte) bool { return false }
func ContainsRune(s []byte, r rune) bool { return false }
func ContainsAny(s []byte, chars string) bool { return false }
