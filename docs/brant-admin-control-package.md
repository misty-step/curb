# Brant OpenAI Enterprise Admin Control Package

Date: 2026-05-18

## Purpose

Brant needs OpenAI Enterprise controls that make runaway agent usage visible,
bounded, and attributable at the user and role level. The immediate incident was
a Codex session that continued from Friday, May 15, 2026 to Monday, May 18,
2026, consuming more credits than intended. The control response should not
depend on people remembering to stop sessions before leaving work.

This package focuses on native Brant-side controls:

- RBAC roles for users and contractors.
- Weekly usage hard caps and alert thresholds.
- Codex managed configuration.
- Analytics and compliance exports.
- Alerting and recurring review.

## Executive Recommendation

Create a dedicated `Contractor - Agentic Engineering` role in ChatGPT
Enterprise and assign all consulting-company users to it. Configure that role
with conservative weekly usage hard caps, admin alert thresholds, and managed
Codex settings. Use Codex Analytics and Compliance exports to build recurring
visibility into user-level usage, long-running activity, high-credit users, and
weekend or after-hours agent work.

The core operating principle is straightforward:

> Users may be trusted with powerful agentic tools, but no user should have
> unbounded access by default.

## Native Controls To Enable

### 1. Contractor RBAC Role

Set up one or more dedicated custom roles for non-employee or high-variance
users:

- `Contractor - Agentic Engineering`
- `Employee - Agentic Engineering`
- `Power User - Agentic Engineering`
- `Restricted / Trial Agent User`

The contractor role should be the default for external engineering operators
working inside Brant's Enterprise workspace. Avoid assigning contractors to any
secondary role with more permissive usage settings, because OpenAI documents
that the most permissive assigned role can effectively win.

Control objective:

- Make contractor usage separable from employee usage.
- Allow lower hard caps for contractors without reducing employee productivity.
- Make reporting by role and group operationally meaningful.

Source: OpenAI documents per-user, per-week usage limits through RBAC custom
roles in ChatGPT Enterprise, configurable by workspace owners.
https://help.openai.com/en/articles/20001001

### 2. Weekly Usage Limits And Hard Caps

Configure weekly per-user credit limits for each role.

Recommended starting policy:

| Role | Default action | Rationale |
| --- | --- | --- |
| Contractor - Agentic Engineering | Hard cap plus admin alert | External users should not be unbounded consumers of Brant credits. |
| Employee - Agentic Engineering | Admin alert, optional hard cap | Preserve productivity while measuring normal usage. |
| Power User - Agentic Engineering | Higher alert threshold, reviewed monthly | Some users create outsized value and need headroom. |
| Restricted / Trial Agent User | Low hard cap | Safe onboarding and experimentation. |

The exact cap should be set from Brant's contract, credit pool, team size, and
expected usage pattern. A practical first cut is to pick a contractor weekly cap
that is low enough to make a 40-hour runaway impossible to repeat at full blast,
then revise after two weeks of measured usage.

Control objective:

- Prevent one user's runaway session from consuming a disproportionate share of
  the shared Enterprise credit pool.
- Create a visible policy line between normal agent use and exceptional usage.
- Force explicit escalation for unusually expensive work.

Source: OpenAI describes Admin alerts and Hard caps for Enterprise role usage
limits. Hard caps block advanced model usage after the weekly role limit is
reached.
https://help.openai.com/en/articles/20001001

### 3. User-Visible Usage Status

Preferred user experience:

- Each user can see their current role, weekly limit, consumed credits, and
  remaining credits.
- Each user receives warning banners or messages before they hit a hard cap.
- Users can request temporary elevation with a work item, reason, and duration.

Current documented limitation:

OpenAI's usage-limit article says members cannot view or edit their limits and
only see a generic message when they hit a hard cap. Admin alerts are admin-only
and do not proactively warn users.

Recommendation:

- Ask OpenAI account support whether Brant can expose user-visible credit
  status natively.
- If not, build an internal dashboard or weekly digest from Codex Analytics API
  exports for Brant admins and, where policy allows, individual users.
- Treat user-visible status as a UX improvement, not the enforcement mechanism.

Source: OpenAI usage-limit FAQ.
https://help.openai.com/en/articles/20001001

### 4. Codex Managed Configuration

Use managed Codex configuration for contractor and engineering roles.

Recommended baseline:

- Disallow `danger-full-access` / yolo-style operation.
- Disallow `approval_policy = "never"` for ordinary interactive client work.
- Restrict sandbox modes to `read-only` and `workspace-write` unless explicitly
  approved.
- Keep network access off by default; require documented exceptions.
- Restrict MCP servers and app/tool access to approved integrations.
- Configure command rules for high-risk shell entrypoints.
- Configure managed hooks for telemetry and policy checks.

Important limitation:

Managed configuration is powerful, but it should not be the only control. OpenAI
documents that when managed requirements cannot be fetched and no valid cache is
available, Codex can continue without that managed requirements layer. Brant
should pair cloud-managed configuration with endpoint management where possible.

Sources:

- Codex Admin Setup:
  https://developers.openai.com/codex/enterprise/admin-setup
- Codex Managed Configuration:
  https://developers.openai.com/codex/enterprise/managed-configuration

### 5. Codex Analytics Export

Enable and operationalize Codex Analytics.

Use cases:

- Weekly user-level usage review.
- Contractor role usage report.
- Top users by credits, turns, and tokens.
- Weekend and after-hours activity review.
- Per-client surface breakdown: desktop, CLI, IDE extension, cloud, code review.
- Trend detection for sudden spikes.

Alert candidates:

- User exceeds 50 percent of weekly limit.
- User exceeds 80 percent of weekly limit.
- User enters top 5 percent of weekly credit usage.
- Weekend activity from contractor role.
- Unusual thread/turn count for a user.
- Credits used without linked work item or engagement context.

Important limitation:

OpenAI documents that Codex usage data can lag by up to 12 hours. Analytics are
excellent for governance and cost monitoring, but not sufficient as the only
real-time runaway stop.

Source: Codex Governance.
https://developers.openai.com/codex/enterprise/governance

### 6. Compliance Export

Enable Compliance API export into Brant's security, audit, or data-retention
pipeline.

Use cases:

- Incident reconstruction.
- Audit trails for who ran what, when, using which model.
- Token usage and request metadata review.
- Retention beyond OpenAI's short compliance-log window.
- SIEM/DLP/eDiscovery integration.

OpenAI documents that Codex Compliance API exports can include prompt text,
responses, workspace/user/timestamp/model identifiers, token usage, and request
metadata for ChatGPT-authenticated Codex activity. It also documents that these
audit logs are retained for up to 30 days, so Brant should continuously export
them if longer retention is required.

Sources:

- Codex Governance:
  https://developers.openai.com/codex/enterprise/governance
- OpenAI Compliance Platform:
  https://help.openai.com/en/articles/9261474-openai-compliance-platform-for-enterprise-customers/

## Alerting Model

Brant should use two classes of alerting.

### Spend Alerts

Spend alerts should be based on credits and role-level thresholds:

- Contractor user crosses 50 percent of weekly cap.
- Contractor user crosses 80 percent of weekly cap.
- Contractor user hits hard cap.
- Contractor group crosses expected weekly aggregate usage.
- Any user has a single-day spike above baseline.

### Operational Alerts

Operational alerts should be based on behavior:

- Agent usage occurs over the weekend.
- Agent usage occurs after local business hours.
- A user has unusually high thread/turn counts.
- Codex usage continues while no work item or ticket is linked.
- Compliance logs show repeated retries, repeated compactions, or unusually long
  conversations.

Native analytics may not be real-time enough for every operational alert. Brant
should still implement these alerts for governance, and contractor-owned local
watchdogs should handle immediate kill behavior on managed laptops.

## Operating Process

### First 24 Hours

- Preserve incident evidence for May 15-18, 2026.
- Export Codex Analytics and Compliance logs for the user and workspace window.
- Identify the user's roles, seat type, usage limits, and Codex settings.
- Temporarily assign contractors to a stricter role if no such role exists.

### First 3 Business Days

- Create contractor-specific RBAC role.
- Configure weekly hard caps and admin alerts.
- Enable analytics export and define alert thresholds.
- Confirm Compliance API export retention path.
- Draft exception process for temporary cap increases.

### First 2 Weeks

- Review contractor usage twice weekly.
- Tune hard caps using observed normal work.
- Identify power users separately instead of widening contractor defaults.
- Decide whether user-visible usage status requires an internal dashboard.

## Open Questions For Brant / OpenAI

- Can Brant expose individual usage consumed/remaining to end users natively?
- Can Brant receive near-real-time alerts for per-user Codex credit velocity, or
  only delayed analytics?
- Which Codex managed-configuration fields are available in Brant's tenant
  today?
- Are contractors using ChatGPT-authenticated Codex only, or API-key
  authenticated workflows too?
- Does Brant already export Compliance API logs continuously, and where are they
  retained?
- What is the desired weekly contractor budget per person?

## Success Criteria

This control package is successful when:

- Brant can identify high-usage users and contractor usage by role.
- A single contractor cannot run unbounded against the shared credit pool.
- Workspace owners receive alerts before abnormal usage becomes expensive.
- Brant can reconstruct a Codex incident from exported logs.
- Users have a clear path to request temporary higher limits for valid work.

