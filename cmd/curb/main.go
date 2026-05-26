package main

import (
	"context"
	"fmt"
	"os"

	"github.com/phaedrus/curb/internal/platform"
)

func main() {
	if err := run(os.Args); err != nil {
		fmt.Fprintln(os.Stderr, "curb:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	return runWithDeps(args, platform.Capture, platform.Notify)
}

func runWithDeps(args []string, capture processCapture, notify notifier) error {
	if len(args) < 2 {
		if _, err := ensureDefaultConfig(false); err != nil {
			return err
		}
		return cmdWatch(nil)
	}
	switch args[1] {
	case "help", "-h", "--help":
		if len(args) > 2 && args[2] == "advanced" {
			usageAdvanced()
			return nil
		}
		usage()
		return nil
	case "init":
		return cmdInit(args[2:])
	case "install":
		return cmdInstall(args[2:])
	case "config":
		return cmdConfig(args[2:])
	case "usage":
		return cmdUsage(args[2:])
	case "dashboard", "dash":
		return cmdDashboard(args[2:], capture)
	case "daemon", "api", "serve":
		return cmdDaemon(args[2:], capture)
	case "app":
		return cmdApp(args[2:], capture)
	case "tail":
		return cmdTail(args[2:])
	case "curb", "run", "start", "watch":
		return cmdWatch(args[2:])
	case "scan":
		return cmdScan(args[2:], capture)
	case "validate-config":
		return cmdValidate(args[2:])
	case "status":
		return cmdStatus(args[2:])
	case "runs":
		return cmdRuns(args[2:])
	case "ack":
		return cmdAck(args[2:])
	case "doctor":
		return cmdDoctor(args[2:], capture, notify)
	default:
		usage()
		return fmt.Errorf("unknown command %q", args[1])
	}
}

type processCapture func(context.Context) (*platform.Snapshot, error)
type notifier func(string, string) error

func usage() {
	fmt.Println(`curb

  curb                  start watching
  curb config           configure warnings and limits
  curb dashboard        show live agents and usage
  curb app              serve and open the local dashboard
  curb daemon           serve the local UI/API on loopback
  curb usage            show local agent token usage
  curb tail             stream local usage events
  curb runs             show active runs
  curb install          install to ~/.local/bin/curb

Advanced commands: curb help advanced`)
}

func usageAdvanced() {
	fmt.Println(`curb advanced commands:
  init              create a user config
  install           install this binary to ~/.local/bin/curb
  config            show or update config
  dashboard         show live agents plus recent usage
  app               serve and open the local dashboard
  daemon|api|serve  serve token-gated local API
  usage             summarize local Codex and Claude usage logs
  tail              stream new usage events
  run|start|watch   run the watchdog loop
  scan              print current process matches once
  validate-config   validate config
  status            print config and active run count
  runs              summarize ledger runs
  ack               legacy run-ledger acknowledgement
  doctor            check local capabilities`)
}

func usageConfig() {
	fmt.Println(`curb config commands:
  curb config                         show current config
  curb config path                    print config path
  curb config aggressive              enforcement, warn 30s, kill 60s
  curb config reasonable              alert-only, warn 90m, kill 120m
  curb config observe                 visibility-only
  curb config set --mode alert --warn-after 5m --kill-after 10m
  curb config set --warn-turn-tokens 1000000 --kill-turn-tokens 3000000`)
}
