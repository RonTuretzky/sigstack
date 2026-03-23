# Signal CLI Lock Contention Diagnostic

**Date:** 2026-03-22
**CVM:** `2cc4b01109c79cf81d6470b449c9fd578fabdf1d` (signal-translate-v4)
**Duration analyzed:** ~2 hours (18:22 - 20:23 UTC)

## Summary

Signal CLI's file-based config lock is the primary performance bottleneck. **Every request** to Signal CLI takes 22-120 seconds due to lock contention between the bot's polling loop, the registration proxy, and manual user commands. This is not occasional — it is systemic and constant.

## Raw Numbers

### Request Latency (from GIN access logs)

| Endpoint | Count | Min | Median | Max | Timeouts (2m) |
|----------|-------|-----|--------|-----|----------------|
| `GET /v1/receive/{number}` | 48 | 26.5s | 44.1s | **2m0s** | 12 |
| `GET /v1/accounts` | 38 | 19.3s | 49.4s | **2m0s** | 5 |
| `POST /v1/register/{number}` | 10 | 28.7s | 31.2s | 36.6s | 0 |
| `PUT /v1/groups/...` | 3 | 1m56s | 2m0s | **2m0s** | 2 |
| `PUT /v1/profiles/...` | 3 | 8ms | 2m0s | **2m0s** | 2 |
| `POST /v1/register/.../verify/...` | 2 | 36.3s | 46.6s | 46.6s | 0 |

### Lock Wait Events

Explicit "Config file is in use by another instance, waiting…" log entries: **22 occurrences** in ~90 minutes.

These only represent cases where Signal CLI logged the wait — actual lock contention is higher since many requests simply queue behind the lock without logging.

### Timeout Cascade

The bot polls with a **30-second HTTP timeout** from the Rust client. Signal CLI often takes longer than 30s due to lock contention. This creates a cascading failure:

1. Bot sends `GET /v1/receive` → takes 50s → bot's 30s timeout fires → bot retries
2. Signal CLI is still processing the first request (holding the lock)
3. New request queues behind the lock
4. Requests pile up, each waiting for the previous one
5. Signal CLI eventually responds to the original (now-abandoned) request

The GIN logs show many requests taking exactly **2m0s** — these are Signal CLI's own internal timeout, meaning the request was blocked for the full 2 minutes before giving up.

### Impact on Operations

| Operation | Expected | Actual | Root Cause |
|-----------|----------|--------|------------|
| Registration | 2-5s | 28-37s | Lock wait |
| Verification | 1-2s | 36-47s | Lock wait → code expired (499/401) |
| Accept group invite | <1s | 1m56s-2m0s | Lock wait → timeout → failed |
| Set profile | <1s | 2m0s | Lock wait → timeout → failed |
| Receive messages | <1s | 26-120s | Lock wait |
| List accounts | <1s | 19-120s | Lock wait |

**Registration was only possible** during the window when the bot was NOT deployed (registration-only compose). With the bot running, every operation is degraded to the point of frequent failure.

## Root Cause

Signal CLI uses a **single file lock** (`signal-cli-config/.lock`) for all operations on an account. Every API call — receive, send, register, list accounts, update profile — must acquire this lock exclusively. There is no read/write distinction; even read-only operations like listing accounts take the exclusive lock.

The bot's polling loop (`GET /v1/receive` every ~30s + `GET /v1/accounts` periodically) keeps the lock almost permanently held, starving all other operations.

### Lock contention sources in this deployment:

| Source | Container | Frequency | Lock Duration |
|--------|-----------|-----------|---------------|
| `GET /v1/receive` | signal-bot | Every ~30s | 20-50s per call |
| `GET /v1/accounts` | signal-bot | Every ~60s | 20-50s per call |
| Registration/verify | signal-registration-proxy | On demand | 30-47s per call |
| Group/profile ops | User (manual) | On demand | Blocked entirely |

With a ~30s lock hold time and ~30s polling interval, the lock is held **>90% of the time**, leaving virtually no window for other operations.

## Architecture Recommendations

### Option 1: Use Signal CLI's `json-rpc` mode (Recommended — lowest effort)

Signal CLI supports a `json-rpc` daemon mode (`MODE=json-rpc` in docker env) that keeps a persistent connection and receives messages via push rather than polling. This eliminates the polling loop entirely.

**Changes required:**
- Set `MODE=json-rpc` on signal-api container
- Rewrite `signal-client` to use JSON-RPC over stdin/stdout or TCP instead of REST API
- Remove the polling loop in `receiver.rs`

**Pros:** Eliminates the core contention problem. Messages are pushed, no polling needed.
**Cons:** Requires rewriting the signal-client crate. JSON-RPC protocol is different from REST.

### Option 2: Single-writer queue architecture (Medium effort)

Instead of multiple containers hitting Signal CLI concurrently, funnel all requests through a single serializing proxy:

```
[bot]  ──┐
          ├──> [Request Queue] ──> [Signal CLI]
[proxy]──┘
[user] ──┘
```

**Changes required:**
- Build a lightweight request queue/serializer service
- All Signal CLI access goes through this queue
- Queue prioritizes interactive operations (register, verify, profile) over polling
- Polling requests are deprioritized or coalesced

**Pros:** Eliminates lock contention, enables priority scheduling.
**Cons:** New service to maintain. Adds latency to all operations.

### Option 3: Increase bot timeout + reduce polling frequency (Quick fix)

Increase the bot's HTTP client timeout to 120s (matching the proxy) and reduce polling to every 60-120s when idle.

**Changes required:**
- `crates/signal-client/src/receiver.rs`: Increase receive timeout, add backoff
- `crates/signal-client/src/client.rs`: Increase client timeout to 120s

**Pros:** Stops the timeout cascade. Minimal code changes.
**Cons:** Does NOT fix the underlying problem. Operations are still slow (30-50s each). Just makes them fail less often.

### Option 4: Separate Signal CLI instances (Infrastructure change)

Run two Signal CLI containers sharing the same config volume:
1. One in `json-rpc` mode for receiving messages (bot-only)
2. One in `normal` mode for registration/admin operations (proxy/user)

**Changes required:**
- Add second signal-api container to docker-compose
- Route bot traffic to the json-rpc instance
- Route proxy/admin traffic to the normal instance

**Pros:** Isolates workloads completely.
**Cons:** Signal CLI may not support concurrent access to the same config directory. The file lock exists precisely to prevent this. **Risk of data corruption.**

### Recommended Path

**Short-term (now):** Apply Option 3 — increase timeouts and reduce polling. This unblocks registration and basic operations.

**Medium-term:** Implement Option 1 (json-rpc mode). This is the proper fix and eliminates the fundamental architecture mismatch of polling a file-locked Java process.
