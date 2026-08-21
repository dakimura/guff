package main

import flags "github.com/jessevdk/go-flags"

var _ = flags.Default

// go-flags repeats these by design; upstream exempts them when the file
// imports go-flags.
type Opts struct {
	Mode string `long:"mode" choice:"a" choice:"b"`
	Val  string `long:"val" optional-value:"x" optional-value:"y"`
	Def  string `long:"def" default:"1" default:"2"`
}
