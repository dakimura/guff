package sync

type Mutex struct{}

func (m *Mutex) Lock()   {}
func (m *Mutex) Unlock() {}
