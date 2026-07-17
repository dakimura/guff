package exec

type Cmd struct{}

func Command(name string, arg ...string) *Cmd { return nil }
func CommandContext(ctx interface{}, name string, arg ...string) *Cmd { return nil }
