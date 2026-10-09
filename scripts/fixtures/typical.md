---
name: deploy-checklist
description: Steps and checks for shipping a service release. Use when preparing, verifying or rolling back a deployment.
---

# Deploy checklist

This document is the benchmark's "typical" Markdown file: about the size of the median file on a
developer machine (5–6 KB), with what such files usually contain, namely YAML frontmatter, a few
code fences, a table and lists. Change it and the numbers in the README stop being comparable.

## When to use this

Use the checklist for every production release, including hotfixes. A hotfix skips the staging
soak, but never the smoke test or the rollback plan. If a step does not apply, write down why in
the release ticket instead of skipping it silently.

- The change is merged and the release branch is green in CI.
- The database migrations are reviewed and reversible, or explicitly marked as one-way.
- Someone other than the author is on call for the next four hours.

## Before you start

Check that your tooling is current and that you can reach the cluster. Nothing below works from a
stale checkout.

```bash
git fetch --tags
git checkout release/2026.09
npm ci
npm run build
```

Confirm the version you are about to ship matches the tag, then look at what changed since the
last release:

```bash
git log --oneline v2026.08.3..HEAD
```

## Configuration

Runtime settings come from a JSON file per environment. Keep secrets out of it; they are injected
by the deploy tool from the vault.

```json
{
  "service": "orders",
  "replicas": 4,
  "healthCheck": { "path": "/healthz", "intervalSeconds": 10, "timeoutSeconds": 2 },
  "features": { "newCheckout": true, "legacyExport": false }
}
```

| Environment | Replicas | Soak time | Approval needed |
| ----------- | -------- | --------- | --------------- |
| staging     | 2        | 30 min    | No              |
| canary      | 1        | 15 min    | Release manager |
| production  | 4        | none      | Release manager |

## Rolling out

Roll out to staging first and watch the dashboards for at least the soak time. Look at error rate,
p95 latency and queue depth; a change in any of them is a reason to stop and investigate, not to
wait and see.

1. Deploy to staging and run the smoke test.
2. Wait out the soak time while watching the dashboards.
3. Promote to canary, then to production in two steps of fifty percent each.
4. Post the release notes in the team channel.

The smoke test opens the home page, signs in with the test account, places an order and checks
that the confirmation email arrives. It takes about two minutes and must pass twice in a row.

## Rolling back

Roll back on any of these signals, without waiting for a second opinion: error rate above two
percent for five minutes, p95 latency doubled, or a failed health check on more than one replica.

```bash
deployctl rollback orders --to previous
deployctl status orders --watch
```

After a rollback, write down what you saw and when. The notes matter more than the fix, because
the next person deciding whether to roll back will read them first.

## Monitoring

Keep the dashboards open for the first thirty minutes after production is at one hundred percent.
Most regressions show up in the first ten, but slow leaks and cache warm-up problems take longer.
Alerts are routed to the on-call rotation; do not silence one without writing down why.

```bash
deployctl logs orders --since 30m --level error
deployctl metrics orders --window 30m
```

The alert thresholds live next to the service configuration, so they change with the code:

```json
{
  "alerts": [
    { "name": "error-rate", "threshold": 0.02, "forMinutes": 5, "page": true },
    { "name": "latency-p95", "thresholdMs": 800, "forMinutes": 10, "page": true },
    { "name": "queue-depth", "threshold": 5000, "forMinutes": 15, "page": false }
  ]
}
```

## Troubleshooting

Most failed rollouts come from a small set of causes. Check these before asking for help, and
put what you found in the ticket either way.

- A health check that passes locally but fails in the cluster usually means a missing
  environment variable or a network policy; compare the pod's environment with staging.
- A migration that hangs is almost always waiting on a lock held by a long-running query. Find
  the query, decide whether it can be cancelled, and only then retry.
- Replicas that restart in a loop are out of memory or failing their startup probe. The previous
  container's logs, not the current one's, hold the reason.
- A canary that looks healthy but receives no traffic has a routing rule that still points at
  the old version. Compare the rule's target with the version in the release ticket.

## After the release

Close the release ticket with the version, the time of each step, and anything that surprised you.
If a step in this checklist was wrong or missing, fix the checklist in the same pull request.

> **Note**
> Checklists rot. Read this one end to end at the start of every quarter and delete what nobody
> has needed.
