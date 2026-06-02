import { describe, expect, it, vi } from "vitest";
import { fetchOnboarding, fetchSession, fetchSessionTurns } from "./api";

describe("selected-session API client", () => {
  it("fetches selected session detail and rich turn breakdowns from service endpoints", async () => {
    const requests: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const url = String(input);
        requests.push(url);
        if (url.endsWith("/v1/sessions/codex%3Asession%2Fone")) {
          return jsonResponse({ key: "codex:session/one", id: "session/one" });
        }
        if (url.endsWith("/v1/sessions/codex%3Asession%2Fone/turns?limit=20")) {
          return jsonResponse([
            {
              id: "turn-1",
              request_id: "req-1",
              session_key: "codex:session/one",
              provider: "codex",
              input_tokens: 10,
              cached_input_tokens: 2,
              cache_creation_input_tokens: 1,
              output_tokens: 4,
              reasoning_output_tokens: 3,
              total_tokens: 20,
              spent_tokens: 18,
              cumulative_tokens: 40,
              source: "usage-log",
            },
          ]);
        }
        return new Response("not found", { status: 404 });
      }),
    );

    await expect(fetchSession(settings(), "codex:session/one")).resolves.toMatchObject({ id: "session/one" });
    await expect(fetchSessionTurns(settings(), "codex:session/one")).resolves.toMatchObject([
      { input_tokens: 10, reasoning_output_tokens: 3, source: "usage-log" },
    ]);
    expect(requests).toEqual([
      "http://curb.test/v1/sessions/codex%3Asession%2Fone",
      "http://curb.test/v1/sessions/codex%3Asession%2Fone/turns?limit=20",
    ]);
  });
});

describe("readiness API client", () => {
  it("fetches onboarding so first-run readiness is not hidden behind unused endpoints", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        expect(String(input)).toBe("http://curb.test/v1/onboarding");
        return jsonResponse({ required: true, final_sentence: "Curb will notify on high-token turns." });
      }),
    );

    await expect(fetchOnboarding(settings())).resolves.toMatchObject({
      required: true,
      final_sentence: "Curb will notify on high-token turns.",
    });
  });
});

function settings() {
  return { baseUrl: "http://curb.test", token: "secret" };
}

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } });
}
