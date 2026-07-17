package gosec_ok

import (
	"crypto/sha256"
	"hash"
	"net"
	"os/exec"
)

func ok() hash.Hash {
	return sha256.New()
}

func okListen() {
	_, _ = net.Listen("tcp", "127.0.0.1:8080")
}

func okExec() {
	_ = exec.Command("ls", "-la")
}

func okCreds() {
	password := "secret"
	_ = password
	username := "admin"
	_ = username
	txnID := "3637cfcc1eec55a50f78a7c435914583ccbc75a21dec9a0e94dfa077647146d7"
	_ = txnID
}
