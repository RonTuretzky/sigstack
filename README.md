# Signal Bot TEE

Private AI Chat Proxy running in a Trusted Execution Environment (TEE).

This is a fork of [zmanian/signal-bot-tee](https://github.com/zmanian/signal-bot-tee).

## Overview

A Signal bot that runs inside a Dstack-powered TEE (Intel TDX) and proxies user messages to NEAR AI Cloud's private inference API, creating a fully verifiable, end-to-end private AI chat experience.

```
[User] <--Signal E2E--> [TEE: Signal CLI + Bot] <--HTTPS--> [NEAR AI GPU TEE]
                              |
                        [In-memory only]
                        [Intel TDX protected]
```

- **Signal**: E2E encrypted messaging between user and bot
- **Dstack TEE**: Verifiable proxy execution with Intel TDX attestation
- **NEAR AI Cloud**: Private inference with GPU TEE (NVIDIA H100/H200) attestation

## Features

- End-to-end privacy from user device to AI inference
- Dual attestation (Intel TDX + NVIDIA GPU TEE)
- Cryptographic verification with user-provided challenges
- In-memory conversation storage (no external persistence)
- Group chat support with shared conversation context
- Tool use system (calculator, weather, web search)
- Multitenant registration proxy
- Optional credit/payment tracking (x402)

## Bot Commands

| Command | Description |
|---------|-------------|
| `!verify <challenge>` | Get TEE attestation with your challenge embedded in TDX quote |
| `!clear` | Clear conversation history |
| `!models` | List available AI models |
| `!help` | Show help message |

## Quick Start

### Prerequisites

- Rust 1.83+
- Docker & Docker Compose
- Signal phone number (for the bot)
- NEAR AI API key

### Build & Test

```bash
cargo build --release
cargo test
```

### Deploy

```bash
cd docker
cp ../.env.example .env
# Edit .env with your credentials
docker-compose up -d
```

## Project Structure

```
crates/
  signal-bot/                  # Main application binary
  near-ai-client/              # NEAR AI Cloud API client
  conversation-store/          # In-memory conversation storage with TTL
  dstack-client/               # Dstack TEE attestation client
  signal-client/               # Signal CLI REST API client
  signal-registration-proxy/   # Multi-tenant registration service
  tools/                       # Tool use system (calculator, weather, web search)
web/                           # React frontend (Vite + Tailwind)
docker/                        # Docker Compose configs
```

## Documentation

See [CLAUDE.md](./CLAUDE.md) for detailed documentation on:

- Security architecture and TEE trust model
- User verification process
- Registration proxy API
- Deployment to Phala Cloud
- Tool and payment configuration

## License

MIT
