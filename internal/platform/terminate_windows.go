//go:build windows

package platform

func platformSoftTerminate(pid int32) error {
	return hardTerminate(pid)
}
