package strings

func NewReplacer(oldnew ...string) *Replacer { return &Replacer{} }

type Replacer struct{}
