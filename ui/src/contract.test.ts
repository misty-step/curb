import { describe, expect, it } from "vitest";

import configFixture from "../../contracts/api/config.json";
import liveFixture from "../../contracts/api/live.json";
import onboardingFixture from "../../contracts/api/onboarding.json";
import overviewFixture from "../../contracts/api/overview.json";
import readyFixture from "../../contracts/api/ready.json";
import sessionFixture from "../../contracts/api/session.json";
import snapshotFixture from "../../contracts/api/snapshot.json";
import turnsFixture from "../../contracts/api/turns.json";
import { selectDashboard, selectRecovery, selectSessionExplanation } from "./readModel";
import type {
  ConfigView,
  LiveView,
  OnboardingView,
  Overview,
  ReadinessView,
  SessionView,
  Snapshot,
  TurnView,
} from "./types";

describe("shared API contract fixtures", () => {
  it("match the TypeScript API read model shapes", () => {
    expect(snapshotFixture.overview.status).toBe("WATCH");
    expect(sessionFixture.status).toBe("working");
    expect(sessionFixture.alert).toBe("warn");
    const snapshot = snapshotFixture as Snapshot;
    const overview = overviewFixture as Overview;
    const session = sessionFixture as SessionView;
    const turns = turnsFixture as TurnView[];
    const config = configFixture as ConfigView;
    const onboarding = onboardingFixture as OnboardingView;
    const live = liveFixture as LiveView;
    const ready = readyFixture as ReadinessView;

    expect(snapshot.overview).toEqual(overview);
    expect(snapshot.sessions).toEqual([session]);
    expect(snapshot.turns).toEqual([]);
    expect(turns[0]).toMatchObject({
      id: null,
      request_id: null,
      session_key: "codex:session/one",
      input_tokens: 789,
      reasoning_output_tokens: 78,
    });
    expect(config.mode).toBe("alert");
    expect(onboarding.capabilities.process_capture.status).toBe("ready");
    expect(onboarding.recovery[0]).toMatchObject({
      id: "setup",
      command: "curb init --config /tmp/curb/config.yaml",
    });
    expect(live).toEqual({ status: "live", app: "curb", api_version: 1 });
    expect(ready.checks.map((check) => check.name)).toEqual([
      "config",
      "ledger",
      "usage_reader",
      "platform_capabilities",
      "notifications",
      "watcher_runtime",
    ]);
  });

  it("remain renderable through the dashboard selectors", () => {
    const snapshot = snapshotFixture as Snapshot;
    const session = sessionFixture as SessionView;
    const turns = turnsFixture as TurnView[];
    const onboarding = onboardingFixture as OnboardingView;

    const dashboard = selectDashboard(snapshot, 900);
    expect(dashboard.headline).toBe("1 agent past your warn line");
    expect(dashboard.active.map((row) => row.key)).toEqual(["codex:session/one"]);

    const explanation = selectSessionExplanation(session, turns);
    expect(explanation?.turns[0]).toMatchObject({
      label: "turn 1",
      inputTokens: 789,
      reasoningTokens: 78,
      source: "test usage log",
    });

    const recovery = selectRecovery(onboarding, readyFixture as ReadinessView);
    expect(recovery.items.map((item) => item.id)).toEqual(["setup"]);
  });
});
