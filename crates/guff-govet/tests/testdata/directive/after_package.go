package main

// A //go:debug directive is ignored once the package clause has been seen.
// This is the shape that proves the analyzer sees comments past the header:
// the analysis AST drops them, so directive re-parses with PARSE_COMMENTS.

//go:debug asynctimerchan=1

func main() {}
