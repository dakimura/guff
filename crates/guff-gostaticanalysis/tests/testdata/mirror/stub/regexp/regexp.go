package regexp

type Regexp struct{}

func MustCompile(str string) *Regexp { return &Regexp{} }

func (re *Regexp) Match(b []byte) bool                         { return false }
func (re *Regexp) MatchString(s string) bool                   { return false }
func (re *Regexp) FindIndex(b []byte) []int                    { return nil }
func (re *Regexp) FindStringIndex(s string) []int              { return nil }
func (re *Regexp) FindAllIndex(b []byte, n int) [][]int        { return nil }
func (re *Regexp) FindAllStringIndex(s string, n int) [][]int  { return nil }
