//go:build !windows

package platform

import (
	"os"
	"syscall"
)

func platformSoftTerminate(pid int32) error {
	proc, err := os.FindProcess(int(pid))
	if err != nil {
		return err
	}
	return proc.Signal(syscall.SIGTERM)
}
