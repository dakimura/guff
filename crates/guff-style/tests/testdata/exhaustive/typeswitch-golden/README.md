Copies of exhaustive v0.15.0's `check/typeswitch` testdata (MIT, see
`../upstream-v0.15.0/LICENSE`) with `{}` bodies added to the body-less method
declarations: upstream's analysistest tolerates them, but golangci-lint's type
check fails the package ("missing function body") and reports nothing. Nothing
else is changed.
