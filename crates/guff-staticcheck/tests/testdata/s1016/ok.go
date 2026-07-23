package main

type T1 struct{ A int; B string }
type T2 struct{ A int; B string }

type Capabilities struct {
	KubeVersion string
	APIVersions []string
}

func f(x T1) T2 { return T2(x) }

// Pointer receiver copying via &T{...} must not be flagged (upstream skip).
func (c *Capabilities) Copy() *Capabilities {
	return &Capabilities{
		KubeVersion: c.KubeVersion,
		APIVersions: c.APIVersions,
	}
}
