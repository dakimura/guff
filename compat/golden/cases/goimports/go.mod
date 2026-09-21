module example.com/goimportscase

go 1.22

require (
	third.example v1.0.0
	zlocal.example v1.0.0
)

replace third.example => ./third

replace zlocal.example => ./zlocal
