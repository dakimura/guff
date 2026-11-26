package a

type T int

//go:fix inline
type A = T

//go:fix inline
const K = One

const One = 1
