package regexp

type Regexp struct{}

func MustCompile(s string) *Regexp {
	var r Regexp
	return &r
}

func (re *Regexp) FindAll(b []byte, n int) [][]byte { return [][]byte{} }
func (re *Regexp) FindAllIndex(b []byte, n int) [][]int { return [][]int{} }
func (re *Regexp) FindAllString(s string, n int) []string { return []string{} }
func (re *Regexp) FindAllStringIndex(s string, n int) [][]int { return [][]int{} }
func (re *Regexp) FindAllStringSubmatch(s string, n int) [][]string { return [][]string{} }
func (re *Regexp) FindAllStringSubmatchIndex(s string, n int) [][]int { return [][]int{} }
func (re *Regexp) FindAllSubmatch(b []byte, n int) [][]byte { return [][]byte{} }
func (re *Regexp) FindAllSubmatchIndex(b []byte, n int) [][]int { return [][]int{} }
