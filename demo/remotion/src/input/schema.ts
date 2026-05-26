export interface DemoEvidence {
  ledgerPath: string;
  workerLabel: string;
  privacyBoundary: string[];
  phases: Array<{
    label: string;
    command: string;
    expected: string;
  }>;
}

export const defaultEvidence: DemoEvidence = {
  ledgerPath: "demo/006/artifacts/runs.ndjson",
  workerLabel: "Synthetic Sleep",
  privacyBoundary: ["no prompts", "no responses", "no screenshots", "no keystrokes", "no file contents"],
  phases: [
    { label: "observe", command: "curb scan --json", expected: "synthetic worker visible" },
    { label: "warn", command: "curb watch", expected: "warning event" },
    { label: "ack", command: "curb ack <run-id>", expected: "acknowledged event" },
    { label: "enforce", command: "curb watch", expected: "sleep process stopped" },
  ],
};
