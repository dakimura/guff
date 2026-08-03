package main

type R struct{ Complete bool }

func beforeCheck(p *R) {
	_ = p.Complete // want
	if p != nil {
		_ = p
	}
}
